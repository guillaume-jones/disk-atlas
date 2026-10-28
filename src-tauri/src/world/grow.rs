//! Region growing: seeds flood outwards over free cells until each has claimed its budget.
use super::Owners;
use super::mesh::{Cheapest, Mesh, Pt};
use crate::geo::RegionId;
use crate::rng::Rng;
use std::collections::BinaryHeap;
use std::f64::consts::PI;

pub struct Seed {
    pub region: RegionId,
    pub starts: Vec<usize>,
    /// Cells to claim.
    pub budget: f64,
    pub spine: Option<Spine>,
}

const SPINE_SEGMENTS: usize = 5;
/// Segment length and corridor half-width, as fractions of the landmass radius.
const SPINE_STEP: f64 = 0.55;
const SPINE_WIDTH: f64 = 0.55;
/// Largest turn between segments, in radians.
const SPINE_TURN: f64 = 1.9;
/// How much costlier growth gets per (distance from the spine / width)².
const SPINE_PULL: f64 = 4.0;

/// A wandering backbone (a random walk of segments either side of the seed) that a landmass grows along.
pub struct Spine {
    pts: Vec<Pt>,
    width: f64,
}

impl Spine {
    pub fn new(at: Pt, radius: f64, rng: &mut Rng) -> Self {
        let heading = rng.f64() * 2.0 * PI;
        let step = radius * SPINE_STEP;
        let mut pts = vec![at];
        for dir in [0.0, PI] {
            let mut p = at;
            let mut angle = heading + dir;
            for _ in 0..SPINE_SEGMENTS {
                angle += (rng.f64() - 0.5) * SPINE_TURN;
                p = [p[0] + step * angle.cos(), p[1] + step * angle.sin()];
                if dir == 0.0 { pts.push(p) } else { pts.insert(0, p) }
            }
        }
        Spine { pts, width: radius * SPINE_WIDTH }
    }

    /// Cost multiplier for growing into `q`: 1 on the spine, rising with distance from it.
    fn factor(&self, q: Pt) -> f64 {
        let d = self
            .pts
            .windows(2)
            .map(|s| segment_distance(q, s[0], s[1]))
            .fold(f64::MAX, f64::min)
            / self.width;
        1.0 + SPINE_PULL * d * d
    }
}

fn segment_distance(q: Pt, [ax, ay]: Pt, [bx, by]: Pt) -> f64 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((q[0] - ax) * dx + (q[1] - ay) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
    (q[0] - ax - t * dx).hypot(q[1] - ay - t * dy)
}

/// Grows `seeds` simultaneously over cells owned by `avail`, each claiming up to its budget. Cost is normalised by
/// sqrt(budget) so large regions expand proportionally faster, like a power diagram. With `separate`, regions never
/// touch. Returns the cells each seed claimed.
pub fn grow(
    mesh: &Mesh,
    owners: &mut Owners,
    seeds: &[Seed],
    avail: Option<RegionId>,
    separate: bool,
    cost: impl Fn(usize) -> f64,
) -> Vec<usize> {
    let mut heap = BinaryHeap::new();
    let mut claimed = vec![0usize; seeds.len()];
    for (k, seed) in seeds.iter().enumerate() {
        seed.starts.iter().for_each(|&c| heap.push(Cheapest { cost: 0.0, item: (c, k) }));
    }
    while let Some(Cheapest { cost: d, item: (c, k) }) = heap.pop() {
        let seed = &seeds[k];
        if owners[c] != avail || claimed[k] as f64 >= seed.budget {
            continue;
        }
        if separate && mesh.any_neighbour(c, |q| owners[q] != avail && owners[q] != Some(seed.region)) {
            continue;
        }
        owners[c] = Some(seed.region);
        claimed[k] += 1;
        let norm = seed.budget.sqrt().max(1.0);
        for &q in &mesh.nbrs[c] {
            let q = q as usize;
            if owners[q] == avail {
                let shape = seed.spine.as_ref().map_or(1.0, |s| s.factor(mesh.pts[q]));
                heap.push(Cheapest { cost: d + cost(q) * shape / norm, item: (q, k) });
            }
        }
    }
    claimed
}

/// Free cells bordering `region`.
pub fn frontier(mesh: &Mesh, owners: &Owners, region: RegionId, avail: Option<RegionId>) -> Vec<usize> {
    (0..mesh.len())
        .filter(|&c| owners[c] == avail && mesh.any_neighbour(c, |q| owners[q] == Some(region)))
        .collect()
}
