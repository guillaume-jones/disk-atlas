//! Subdividing a region's cells among its children, recursively.
use super::Owners;
use super::grow::{Seed, grow};
use super::mesh::Mesh;
use super::noise::fbm01;
use crate::geo::{Geo, RegionId};
use crate::rng::{Rng, hash_str};
use noise::Perlin;
use std::collections::{BTreeMap, HashSet};
use std::f64::consts::PI;

/// Regions that would get fewer cells than this wait for a finer patch.
const MIN_CELLS: f64 = 6.0;
/// Each seed may start this fraction of the region's length away from its ideal spot, to find a free cell.
const SEED_WINDOW: usize = 30;

struct AxisCell {
    along: f64,
    across: f64,
    cell: usize,
}

pub struct Layout<'a> {
    geo: &'a Geo,
    mesh: &'a Mesh,
    perlin: &'a Perlin,
    /// Region the patch was built for, salting the seed placement.
    root: RegionId,
    /// Regions whose cells were laid out here rather than inherited from a coarser patch.
    pub placed: HashSet<RegionId>,
}

impl<'a> Layout<'a> {
    pub fn new(geo: &'a Geo, mesh: &'a Mesh, perlin: &'a Perlin, root: RegionId) -> Self {
        Layout { geo, mesh, perlin, root, placed: HashSet::new() }
    }

    /// Lays out the children of `region` inside `cells` (its subtree in this patch), then recurses. Children already
    /// present (inherited from a coarser patch) keep their cells.
    pub fn subdivide(&mut self, owners: &mut Owners, region: RegionId, cells: &[usize]) {
        let geo = self.geo;
        let reg = geo.region(region);
        if reg.children.is_empty() || cells.is_empty() || reg.size == 0 {
            return;
        }
        let total = cells.len() as f64;
        let mut groups: BTreeMap<RegionId, Vec<usize>> = BTreeMap::new();
        let mut wild = Vec::new();
        for &c in cells {
            let Some(owner) = owners[c] else { continue };
            if owner == region {
                wild.push(c);
            } else {
                groups.entry(geo.ancestor_at(owner, reg.depth + 1)).or_default().push(c);
            }
        }
        let share = |id: RegionId| geo.region(id).size as f64 / reg.size as f64 * total;
        let mut fresh: Vec<RegionId> = reg
            .children
            .iter()
            .copied()
            .filter(|c| !groups.contains_key(c) && share(*c) >= MIN_CELLS)
            .collect();
        if !fresh.is_empty() && !wild.is_empty() {
            // Seeds sorted by age along the region's long axis, so recent siblings cluster into ridges.
            fresh.sort_by(|a, b| geo.region(*a).mtime.total_cmp(&geo.region(*b).mtime));
            let mut rng = Rng::new(hash_str(&reg.path) ^ u64::from(u32::from(self.root)));
            let order = self.along_long_axis(&wild, &mut rng);
            let spread = order.iter().map(|o| o.across).fold(0.0, f64::max);
            let budget_sum: f64 = fresh.iter().map(|&c| share(c)).sum();
            let window = (order.len() / SEED_WINDOW).max(2);
            let mut taken = HashSet::new();
            let mut seeds = Vec::new();
            let mut acc = 0.0;
            for &id in &fresh {
                let budget = share(id);
                let f = ((acc + budget / 2.0) / budget_sum).clamp(0.0, 1.0);
                acc += budget;
                let mid = (f * (order.len() - 1) as f64) as usize;
                let target = rng.f64() * 0.5 * spread;
                let (lo, hi) = (mid.saturating_sub(window), (mid + window).min(order.len() - 1));
                let pick = (lo..=hi)
                    .filter(|&i| !taken.contains(&order[i].cell))
                    .min_by(|&i, &j| (order[i].across - target).abs().total_cmp(&(order[j].across - target).abs()))
                    .map(|i| order[i].cell)
                    .or_else(|| order.iter().map(|o| o.cell).find(|c| !taken.contains(c)));
                if let Some(c) = pick {
                    taken.insert(c);
                    seeds.push(Seed { region: id, starts: vec![c], budget, spine: None });
                }
            }
            let mesh = self.mesh;
            let perlin = self.perlin;
            let region_freq = 1.0 / (total.sqrt() * mesh.spacing * 0.6);
            let cell_freq = 1.0 / (mesh.spacing * 5.0);
            let offset = (hash_str(&reg.path) % 1000) as f64;
            // Region-scale noise shapes the territory; cell-scale noise breaks up the hex lattice on small regions.
            grow(mesh, owners, &seeds, Some(region), false, |c| {
                let [x, y] = mesh.pts[c];
                0.3 + 1.4 * fbm01(perlin, x * region_freq + offset, y * region_freq, 3)
                    + 0.9 * fbm01(perlin, x * cell_freq - offset, y * cell_freq, 2)
            });
            self.placed.extend(seeds.iter().map(|s| s.region));
            for &c in &wild {
                if let Some(owner) = owners[c].filter(|&o| o != region) {
                    groups.entry(owner).or_default().push(c);
                }
            }
        }
        for (child, child_cells) in groups {
            self.subdivide(owners, child, &child_cells);
        }
    }

    /// `cells` projected onto their principal axis (pointing either way), sorted along it.
    fn along_long_axis(&self, cells: &[usize], rng: &mut Rng) -> Vec<AxisCell> {
        let pts = &self.mesh.pts;
        let n = cells.len() as f64;
        let (sx, sy) = cells.iter().fold((0.0, 0.0), |(a, b), &c| (a + pts[c][0], b + pts[c][1]));
        let (mx, my) = (sx / n, sy / n);
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for &c in cells {
            let (dx, dy) = (pts[c][0] - mx, pts[c][1] - my);
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        let flip = if rng.f64() < 0.5 { PI } else { 0.0 };
        let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy) + flip;
        let (ax, ay) = (angle.cos(), angle.sin());
        let mut order: Vec<AxisCell> = cells
            .iter()
            .map(|&c| {
                let (dx, dy) = (pts[c][0] - mx, pts[c][1] - my);
                AxisCell {
                    along: dx * ax + dy * ay,
                    across: (dy * ax - dx * ay).abs(),
                    cell: c,
                }
            })
            .collect();
        order.sort_by(|a, b| a.along.total_cmp(&b.along));
        order
    }
}
