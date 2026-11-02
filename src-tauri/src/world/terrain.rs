//! Elevation: sea depth from the shore, land height from file recency (recent data rises into mountains), and
//! hill shading.
use super::mesh::{Mesh, Pt};
use super::noise::{coast_wiggle, fbm01, mountains, octaves, smoothstep};
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
/// Lowest world land and highest world sea. Shoreline noise stays inside this margin, so it reshapes the coast between
/// land and sea cells without flipping any: the continents keep their shapes.
const MIN_LAND: f64 = 0.02;
const MAX_SEA: f64 = -MIN_LAND;
const SHORE_NOISE: f64 = 0.015;
/// Scales noise, which rarely nears its extremes, so a typical coast moves by most of `SHORE_NOISE`.
const SHORE_NOISE_GAIN: f64 = 2.5;
/// Height change across a shoreline cell. Gentle, so that noise moves the coast by a good part of a cell.
const SHORE_SLOPE: f32 = 0.05;
/// Steps along a line searched for the coast, and bisections then spent on the one that crosses it.
const SHORE_STEPS: u32 = 12;
const SHORE_BISECTIONS: u32 = 6;
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
    /// Height before shoreline noise, which finer meshes interpolate.
    pub ground: Vec<f32>,
    /// Height with shoreline noise; the coast is where it crosses zero.
    pub elevation: Vec<f32>,
}

/// The one coast: `ground` at `pt` with shoreline noise, the same at any resolution. Positive on land.
pub fn shoreline(perlin: &Perlin, pt: Pt, ground: f64) -> f64 {
    let shore = 1.0 - smoothstep(0.6 * MIN_LAND, MIN_LAND, ground.abs());
    if shore == 0.0 {
        return ground;
    }
    ground + SHORE_NOISE * shore * (SHORE_NOISE_GAIN * coast_wiggle(perlin, pt[0], pt[1])).clamp(-1.0, 1.0)
}

/// Where the straight line from land `a` to sea `b`, with ground heights linear along it, meets the coast.
pub fn shore_between(perlin: &Perlin, (a, ground_a): (Pt, f32), (b, ground_b): (Pt, f32)) -> Pt {
    let at = |t: f64| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
    let height = |t: f64| shoreline(perlin, at(t), f64::from(ground_a) + t * f64::from(ground_b - ground_a));
    let step = 1.0 / f64::from(SHORE_STEPS);
    let Some(wet) = (1..=SHORE_STEPS).map(|i| f64::from(i) * step).find(|&t| height(t) <= 0.0) else {
        return b;
    };
    let (mut dry, mut wet) = (wet - step, wet);
    for _ in 0..SHORE_BISECTIONS {
        let middle = dry.midpoint(wet);
        if height(middle) > 0.0 {
            dry = middle;
        } else {
            wet = middle;
        }
    }
    at(dry.midpoint(wet))
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
    let mut ground: Vec<f32> = (0..mesh.len())
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
            let smoothed = (landness[c] - 0.5) * SHORE_SLOPE * (1.0 - keep) + ground[c] * keep;
            ground[c] = if land[c] { smoothed.max(MIN_LAND as f32) } else { smoothed.min(MAX_SEA as f32) };
        }
    }
    let elevation = (0..mesh.len())
        .into_par_iter()
        .map(|c| shoreline(perlin, mesh.pts[c], f64::from(ground[c])) as f32)
        .collect();
    Terrain { recency, ground, elevation }
}

/// Ground, recency, elevation and relief per cell.
type Columns = (Vec<f32>, (Vec<f32>, (Vec<f32>, Vec<f32>)));

/// Recency of a region patch, whose regions differ from its parent's.
pub fn patch_recency(geo: &Geo, mesh: &Mesh, owners: &Owners) -> Vec<f32> {
    let mut recency = recency(geo, owners);
    mesh.diffuse(&mut recency, &mesh.inside, RECENCY_SMOOTHING);
    recency
}

/// Terrain for a finer patch: the parent's surface interpolated, plus the octaves the parent couldn't resolve and
/// `own` recency, if it isn't just the parent's. Also returns the relief to hill-shade, which leaves that recency out:
/// its step is a set height over a set number of cells, so it would grow steeper on every finer mesh.
pub fn refine(
    mesh: &Mesh,
    ice: &[bool],
    world: &Patch,
    parents: &[&Patch],
    located: &[Option<Located>],
    perlin: &Perlin,
    own: Option<&[f32]>,
) -> (Terrain, Vec<f32>) {
    let to_edge = mesh.bfs(|c| mesh.edge[c], |c| mesh.inside[c]);
    let detail_octaves = octaves(mesh.spacing);
    let (ground, (recency, (elevation, relief))): Columns = (0..mesh.len())
        .into_par_iter()
        .map(|c| {
            let Some(Located { parent: pi, cell: q }) = located[c] else {
                return (0.0, (0.0, (0.0, 0.0)));
            };
            let parent = parents[pi];
            let pt = mesh.pts[c];
            let (e, parent_recency) = parent.surface_at(q, pt);
            let r = own.map_or(parent_recency, |recency| f64::from(recency[c]));
            // Parents interpolate slightly differently, so the shore reads the world directly: one coast at every level.
            let e = if e.abs() < 3.0 * MIN_LAND {
                let shore = world.ground_at(pt);
                shore + smoothstep(MIN_LAND, 2.0 * MIN_LAND, shore.abs()) * (e - shore)
            } else {
                e
            };
            if e <= 0.0 || ice[c] {
                return (e as f32, (r as f32, (shoreline(perlin, pt, e) as f32, e as f32)));
            }
            // Detail stays off the shore, which only `shoreline` shapes, so the coast is the same at every level.
            let inland = smoothstep(MIN_LAND, 2.0 * MIN_LAND, e);
            let taper = smoothstep(0.0, EDGE_TAPER_CELLS, f64::from(to_edge[c]));
            let parent_octaves = octaves(parent.mesh.spacing);
            let surface = e + inland * mountains(perlin, pt[0], pt[1], parent_octaves, detail_octaves, r);
            let ground = (surface + inland * taper * 0.3 * (r - parent_recency)).max(e * 0.5);
            let relief = surface.max(e * 0.5);
            (ground as f32, (r as f32, (shoreline(perlin, pt, ground) as f32, relief as f32)))
        })
        .unzip();
    (Terrain { recency, ground, elevation }, relief)
}

/// Kept triangles around each vertex, flattened: vertex v owns `tris[start[v]..start[v + 1]]`.
pub struct VertexTriangles {
    start: Vec<u32>,
    tris: Vec<u32>,
}

impl VertexTriangles {
    pub fn new(mesh: &Mesh) -> Self {
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
    pub fn barycentric(&self, mesh: &Mesh, q: usize, pt: Pt) -> Option<[(usize, f64); 3]> {
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

/// Lambertian hill shading from the north-west, using each cell's least-squares slope. Heights below the shore's are
/// shaded as flat, so the coarse step up from the sea doesn't show.
pub fn shade(mesh: &Mesh, elevation: &[f32]) -> Vec<f32> {
    let sun_len = (SUN[0] * SUN[0] + SUN[1] * SUN[1] + SUN[2] * SUN[2]).sqrt();
    let floor = (2.0 * MIN_LAND) as f32;
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
                let de = f64::from(elevation[q].max(floor) - elevation[c].max(floor));
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
