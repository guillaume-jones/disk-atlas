//! Elevation: sea depth from the shore, land height from file recency (recent data rises into mountains), and
//! hill shading.
use super::mesh::{Mesh, Pt};
use super::noise::{fbm01, mountains, octaves, smoothstep};
use super::{Located, Owners, Patch};
use crate::geo::{Geo, RegionId, RegionKind};
use noise::{NoiseFn, Perlin};
use rayon::prelude::*;
use std::collections::HashSet;

const SECONDS_PER_DAY: f64 = 86_400.0;
/// Recency decays over two horizons: most of it within months, a slow tail so data older than a year still
/// separates (1 year vs 2+ years).
const RECENT_DAYS: f64 = 60.0;
const TAIL_DAYS: f64 = 400.0;
const RECENT_WEIGHT: f64 = 0.85;
const TAIL_WEIGHT: f64 = 0.15;
const RECENCY_SMOOTHING: usize = 8;
/// Sea floor reaches full depth this many cells offshore.
const SHELF_CELLS: f64 = 30.0;
const MAX_SEA_DISTANCE: u32 = 60;
/// Land rises to full height this many cells inland.
const COAST_RAMP_CELLS: f64 = 8.0;
/// Lowest land and highest sea, so the shoreline never sits on a cell centre.
const MIN_LAND: f64 = 0.006;
const MAX_SEA: f64 = -0.01;
/// Fine patches blend into their parent's terrain over this many cells from their edge.
const EDGE_TAPER_CELLS: f64 = 12.0;
const SUN: [f64; 3] = [-1.0, -1.0, 1.4];
const SHADE_RELIEF: f64 = 0.06;

/// Regions with at least one descendant holding cells in this patch.
pub fn regions_with_children(geo: &Geo, owners: &Owners) -> HashSet<RegionId> {
    let mut set = HashSet::new();
    for &owner in owners.iter().flatten() {
        let mut ancestor = geo.region(owner).parent;
        while let Some(a) = ancestor {
            if !set.insert(a) {
                break;
            }
            ancestor = geo.region(a).parent;
        }
    }
    set
}

/// exp(-age/τ) per cell: 1 for today, ~0 after a year. A region's own cells use its dust's age once children are drawn.
fn recency(geo: &Geo, owners: &Owners) -> Vec<f32> {
    let parents = regions_with_children(geo, owners);
    owners
        .iter()
        .map(|&owner| {
            let Some(r) = owner else { return 0.0 };
            let reg = geo.region(r);
            let mtime = if parents.contains(&r) && reg.dust.has_files() {
                reg.dust.mtime()
            } else {
                reg.mtime
            };
            let age = ((geo.now - mtime) / SECONDS_PER_DAY).max(0.0);
            (RECENT_WEIGHT * (-age / RECENT_DAYS).exp() + TAIL_WEIGHT * (-age / TAIL_DAYS).exp()) as f32
        })
        .collect()
}

pub struct Terrain {
    pub recency: Vec<f32>,
    pub elevation: Vec<f32>,
}

/// World terrain from scratch: sea shelves, ice sheets and recency-driven mountains.
pub fn world(geo: &Geo, mesh: &Mesh, owners: &Owners, ice: &[bool], perlin: &Perlin) -> Terrain {
    let water: Vec<bool> = owners
        .iter()
        .map(|&r| r.is_none_or(|r| geo.region(r).kind == RegionKind::Ocean))
        .collect();
    let land: Vec<bool> = water.iter().map(|w| !w).collect();
    let mut recency = recency(geo, owners);
    mesh.diffuse(&mut recency, &land, RECENCY_SMOOTHING);
    let to_coast = mesh.bfs(|c| land[c] && mesh.any_neighbour(c, |q| water[q]), |c| land[c]);
    let to_sea = mesh.bfs(|c| water[c] && mesh.any_neighbour(c, |q| land[q]), |c| water[c]);
    let detail_octaves = octaves(mesh.spacing);
    let mut elevation: Vec<f32> = (0..mesh.len())
        .into_par_iter()
        .map(|c| {
            let [x, y] = mesh.pts[c];
            let e = if water[c] {
                let offshore = f64::from(to_sea[c].min(MAX_SEA_DISTANCE));
                (-(0.03 + 0.97 * (offshore / SHELF_CELLS).min(1.0)) + 0.03 * perlin.get([x * 20.0, y * 20.0])).min(MAX_SEA)
            } else {
                let falloff = 0.2 + 0.8 * smoothstep(0.0, COAST_RAMP_CELLS, f64::from(to_coast[c]));
                let r = f64::from(recency[c]);
                let base = if ice[c] {
                    0.14 + 0.3 * fbm01(perlin, x * 6.0, y * 6.0 + 9.0, 5)
                } else {
                    0.04 + 0.3 * r + mountains(perlin, x, y, 0, detail_octaves, r)
                };
                (base * falloff).max(MIN_LAND)
            };
            e as f32
        })
        .collect();
    // Shoreline cells take their height from a smoothed land/sea field, so the drawn coast curves between cell
    // centres instead of stepping along the lattice.
    let mut landness: Vec<f32> = land.iter().map(|&l| f32::from(u8::from(l))).collect();
    mesh.diffuse(&mut landness, &mesh.inside, 2);
    for c in 0..mesh.len() {
        let d = if land[c] { to_coast[c] } else { to_sea[c] };
        if d <= 1 {
            let keep = if d == 0 { 0.0 } else { 0.5 };
            elevation[c] = (landness[c] - 0.5) * 0.12 * (1.0 - keep) + elevation[c] * keep;
        }
    }
    Terrain { recency, elevation }
}

/// Terrain for a finer patch: the parent's surface interpolated, plus the octaves the parent couldn't resolve and
/// this patch's own recency.
pub fn refine(
    geo: &Geo,
    mesh: &Mesh,
    owners: &Owners,
    ice: &[bool],
    parents: &[&Patch],
    located: &[Option<Located>],
    perlin: &Perlin,
) -> Terrain {
    let mut recency = recency(geo, owners);
    mesh.diffuse(&mut recency, &mesh.inside, RECENCY_SMOOTHING);
    let to_edge = mesh.bfs(|c| mesh.edge[c], |c| mesh.inside[c]);
    let detail_octaves = octaves(mesh.spacing);
    let mut used = vec![false; parents.len()];
    located.iter().flatten().for_each(|l| used[l.parent] = true);
    let corners: Vec<Option<VertexTriangles>> = parents
        .par_iter()
        .zip(&used)
        .map(|(parent, &used)| used.then(|| VertexTriangles::new(&parent.mesh)))
        .collect();
    let elevation = (0..mesh.len())
        .into_par_iter()
        .map(|c| {
            let Some(Located { parent: pi, cell: q }) = located[c] else {
                return 0.0;
            };
            let parent = parents[pi];
            let pt = mesh.pts[c];
            let (e, parent_recency) = match corners[pi].as_ref().and_then(|vt| vt.barycentric(&parent.mesh, q, pt)) {
                Some(weights) => weights.iter().fold((0.0, 0.0), |(e, r), &(k, w)| {
                    (
                        e + w * f64::from(parent.terrain.elevation[k]),
                        r + w * f64::from(parent.terrain.recency[k]),
                    )
                }),
                None => (f64::from(parent.terrain.elevation[q]), f64::from(parent.terrain.recency[q])),
            };
            if e <= 0.0 || ice[c] {
                return e as f32;
            }
            let r = f64::from(recency[c]);
            let taper = smoothstep(0.0, EDGE_TAPER_CELLS, f64::from(to_edge[c]));
            let parent_octaves = octaves(parent.mesh.spacing);
            (e + taper * 0.3 * (r - parent_recency) + mountains(perlin, pt[0], pt[1], parent_octaves, detail_octaves, r)).max(e * 0.5)
                as f32
        })
        .collect();
    Terrain { recency, elevation }
}

/// Kept triangles around each vertex, flattened: vertex v owns `tris[start[v]..start[v + 1]]`.
struct VertexTriangles {
    start: Vec<u32>,
    tris: Vec<u32>,
}

impl VertexTriangles {
    fn new(mesh: &Mesh) -> Self {
        let mut start = vec![0u32; mesh.len() + 1];
        mesh.tris.iter().for_each(|&v| start[v as usize + 1] += 1);
        for v in 0..mesh.len() {
            start[v + 1] += start[v];
        }
        let mut fill = start.clone();
        let mut tris = vec![0u32; mesh.tris.len()];
        for (i, &v) in mesh.tris.iter().enumerate() {
            tris[fill[v as usize] as usize] = (i / 3) as u32;
            fill[v as usize] += 1;
        }
        VertexTriangles { start, tris }
    }

    fn around(&self, v: usize) -> &[u32] {
        &self.tris[self.start[v] as usize..self.start[v + 1] as usize]
    }

    /// Corners and barycentric weights of the triangle near vertex `q` that contains `pt`, matching the GPU's
    /// interpolation of the coarse mesh.
    fn barycentric(&self, mesh: &Mesh, q: usize, pt: Pt) -> Option<[(usize, f64); 3]> {
        for v in std::iter::once(q).chain(mesh.nbrs[q].iter().map(|&k| k as usize)) {
            for &tri in self.around(v) {
                let first = tri as usize * 3;
                let corners: [usize; 3] = [mesh.tris[first], mesh.tris[first + 1], mesh.tris[first + 2]].map(|i| i as usize);
                let [[x0, y0], [x1, y1], [x2, y2]] = corners.map(|k| mesh.pts[k]);
                let det = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
                let w0 = ((y1 - y2) * (pt[0] - x2) + (x2 - x1) * (pt[1] - y2)) / det;
                let w1 = ((y2 - y0) * (pt[0] - x2) + (x0 - x2) * (pt[1] - y2)) / det;
                let w2 = 1.0 - w0 - w1;
                if w0 >= -1e-9 && w1 >= -1e-9 && w2 >= -1e-9 {
                    return Some([(corners[0], w0), (corners[1], w1), (corners[2], w2)]);
                }
            }
        }
        None
    }
}

/// Lambertian hill shading from the north-west, using each cell's least-squares slope.
pub fn shade(mesh: &Mesh, elevation: &[f32]) -> Vec<f32> {
    let sun_len = (SUN[0] * SUN[0] + SUN[1] * SUN[1] + SUN[2] * SUN[2]).sqrt();
    (0..mesh.len())
        .into_par_iter()
        .map(|c| {
            if !mesh.inside[c] {
                return 0.0;
            }
            let (mut sxx, mut sxy, mut syy, mut sxe, mut sye) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for &q in &mesh.nbrs[c] {
                let q = q as usize;
                let (dx, dy) = (mesh.pts[q][0] - mesh.pts[c][0], mesh.pts[q][1] - mesh.pts[c][1]);
                let de = f64::from(elevation[q] - elevation[c]);
                sxx += dx * dx;
                sxy += dx * dy;
                syy += dy * dy;
                sxe += dx * de;
                sye += dy * de;
            }
            let det = sxx * syy - sxy * sxy;
            let (gx, gy) = if det.abs() < 1e-30 {
                (0.0, 0.0)
            } else {
                ((syy * sxe - sxy * sye) / det, (sxx * sye - sxy * sxe) / det)
            };
            let (nx, ny, nz) = (-gx * SHADE_RELIEF, -gy * SHADE_RELIEF, 1.0);
            let normal_len = (nx * nx + ny * ny + nz * nz).sqrt();
            ((nx * SUN[0] + ny * SUN[1] + nz * SUN[2]) / (normal_len * sun_len)).max(0.0) as f32
        })
        .collect()
}
