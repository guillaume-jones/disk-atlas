//! Line overlays: Voronoi borders between regions and rivers draining the land, both hand-drawn with midpoint
//! displacement.
use super::Owners;
use super::mesh::{Cheapest, Dual, Mesh, Pt, circumcenter};
use super::terrain::shore_between;
use crate::geo::{Geo, RegionId, RegionKind};
use delaunator::EMPTY;
use noise::{NoiseFn, Perlin};
use std::collections::{BinaryHeap, HashSet};

const BORDER_SUBDIVISIONS: u32 = 2;
const RIVER_SUBDIVISIONS: u32 = 3;
/// Midpoints move up to this fraction of the segment length sideways.
const WOBBLE: f64 = 0.35;
/// Rivers start where this share of the patch's cells (or at least `MIN_RIVER_FLUX`) drains through a cell.
const RIVER_FLUX_SHARE: f64 = 0.0012;
const MIN_RIVER_FLUX: f64 = 40.0;
/// Higher cells shed more water, so mountain rivers start sooner.
const RAIN_PER_HEIGHT: f64 = 2.0;
/// Minimum drop per step of the priority flood, so flat basins still drain in a fixed direction.
const FLOOD_EPSILON: f64 = 1e-7;

/// A stroke and its weight: the border's depth in the region tree, or the river's relative flow.
pub struct Segment {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub weight: f32,
}

/// Midpoint-displaced polyline from `a` to `b`, so lines look surveyed rather than polygonal.
fn wobbly(out: &mut Vec<Segment>, a: Pt, b: Pt, weight: f32, perlin: &Perlin, subdivisions: u32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if subdivisions == 0 || len < 1e-12 {
        out.push(Segment {
            from: [a[0] as f32, a[1] as f32],
            to: [b[0] as f32, b[1] as f32],
            weight,
        });
        return;
    }
    let k = 0.37 / len;
    let offset = perlin.get([(a[0] + b[0]) * k + 3.3, (a[1] + b[1]) * k - 1.7]) * WOBBLE;
    let mid = [a[0].midpoint(b[0]) - dy * offset, a[1].midpoint(b[1]) + dx * offset];
    wobbly(out, a, mid, weight, perlin, subdivisions - 1);
    wobbly(out, mid, b, weight, perlin, subdivisions - 1);
}

/// Voronoi edges between cells of different land regions, weighted by how deep their regions share ancestry.
/// With `placed`, only borders around those regions are drawn. A border reaching the sea runs on to the coast.
pub fn borders(
    geo: &Geo,
    mesh: &Mesh,
    dual: &Dual,
    owners: &Owners,
    ground: &[f32],
    placed: Option<&HashSet<RegionId>>,
    perlin: &Perlin,
) -> Vec<Segment> {
    let (t, he) = (&dual.triangles, &dual.halfedges);
    let next = |e: usize| if e % 3 == 2 { e - 2 } else { e + 1 };
    let centre = |tri: usize| circumcenter(mesh.pts[t[3 * tri]], mesh.pts[t[3 * tri + 1]], mesh.pts[t[3 * tri + 2]]);
    let mut out = Vec::new();
    for e in 0..he.len() {
        let o = he[e];
        if o == EMPTY || o < e || !dual.kept[e / 3] || !dual.kept[o / 3] {
            continue;
        }
        let (a, b) = (t[e], t[next(e)]);
        let (Some(ra), Some(rb)) = (owners[a], owners[b]) else { continue };
        if !mesh.inside[a] || !mesh.inside[b] || ra == rb {
            continue;
        }
        if [ra, rb]
            .iter()
            .any(|&r| matches!(geo.region(r).kind, RegionKind::Ocean | RegionKind::Isles))
        {
            continue;
        }
        // Depth just below the regions' deepest common ancestor: where they part ways.
        let split = geo.region(geo.common_ancestor(ra, rb)).depth + 1;
        let side = |r: RegionId| (geo.region(r).depth >= split).then(|| geo.ancestor_at(r, split));
        if let Some(placed) = placed
            && !side(ra).is_some_and(|s| placed.contains(&s))
            && !side(rb).is_some_and(|s| placed.contains(&s))
        {
            continue;
        }
        let weight = f32::from(split);
        let sea_past = |side: usize| {
            let far = t[next(next(side))];
            owners[far].is_some_and(|r| geo.region(r).kind == RegionKind::Ocean).then_some(far)
        };
        if sea_past(e).is_none() && sea_past(o).is_none() {
            wobbly(&mut out, centre(e / 3), centre(o / 3), weight, perlin, BORDER_SUBDIVISIONS);
            continue;
        }
        let (pa, pb) = (mesh.pts[a], mesh.pts[b]);
        let middle = [pa[0].midpoint(pb[0]), pa[1].midpoint(pb[1])];
        let middle_ground = ground[a].midpoint(ground[b]);
        for side in [e, o] {
            let end = sea_past(side).map_or_else(
                || centre(side / 3),
                |far| shore_between(perlin, (middle, middle_ground), (mesh.pts[far], ground[far])),
            );
            wobbly(&mut out, middle, end, weight, perlin, BORDER_SUBDIVISIONS);
        }
    }
    out
}

/// Rivers from a priority flood that drains every basin to the coast without carving lakes.
pub fn rivers(mesh: &Mesh, elevation: &[f32], ice: &[bool], perlin: &Perlin) -> Vec<Segment> {
    let is_land = |c: usize| mesh.inside[c] && elevation[c] > 0.0 && !ice[c];
    let mut downstream: Vec<Option<usize>> = vec![None; mesh.len()];
    let mut queued = vec![false; mesh.len()];
    let mut heap = BinaryHeap::new();
    for c in 0..mesh.len() {
        if !is_land(c) {
            continue;
        }
        let lowest_sea = mesh.nbrs[c]
            .iter()
            .map(|&q| q as usize)
            .filter(|&q| elevation[q] <= 0.0)
            .min_by(|&a, &b| elevation[a].total_cmp(&elevation[b]));
        if let Some(sea) = lowest_sea {
            downstream[c] = Some(sea);
            queued[c] = true;
            heap.push(Cheapest { cost: f64::from(elevation[c]), item: c });
        }
    }
    let mut upstream_order = Vec::new();
    while let Some(Cheapest { cost: h, item: c }) = heap.pop() {
        upstream_order.push(c);
        for &q in &mesh.nbrs[c] {
            let q = q as usize;
            if !queued[q] && is_land(q) {
                queued[q] = true;
                downstream[q] = Some(c);
                heap.push(Cheapest {
                    cost: f64::from(elevation[q]).max(h + FLOOD_EPSILON),
                    item: q,
                });
            }
        }
    }
    let mut flux = vec![0.0f64; mesh.len()];
    for &c in upstream_order.iter().rev() {
        flux[c] += 1.0 + RAIN_PER_HEIGHT * f64::from(elevation[c].max(0.0));
        if let Some(d) = downstream[c].filter(|&d| is_land(d)) {
            flux[d] += flux[c];
        }
    }
    let threshold = (mesh.len() as f64 * RIVER_FLUX_SHARE).max(MIN_RIVER_FLUX);
    let mut out = Vec::new();
    for c in 0..mesh.len() {
        let Some(d) = downstream[c] else { continue };
        if !is_land(c) || flux[c] < threshold {
            continue;
        }
        let end = if is_land(d) {
            mesh.pts[d]
        } else {
            shore_between(perlin, (mesh.pts[c], elevation[c]), (mesh.pts[d], elevation[d]))
        };
        wobbly(
            &mut out,
            mesh.pts[c],
            end,
            (flux[c] / threshold).sqrt() as f32,
            perlin,
            RIVER_SUBDIVISIONS,
        );
    }
    out
}
