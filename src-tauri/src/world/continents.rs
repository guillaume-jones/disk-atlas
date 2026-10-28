//! The world map's top level: polar ice bands, continents, islands, scattered isles and the sea.
use super::grow::{Seed, Spine, frontier, grow};
use super::mesh::{Mesh, Pt, UNREACHED};
use super::noise::{fbm01, roughness, smoothstep};
use super::{WORLD_H, WORLD_W};
use crate::geo::{Geo, RegionId, RegionKind};
use crate::rng::{Rng, hash_str};
use noise::{NoiseFn, Perlin};
use std::collections::{HashMap, VecDeque};
use std::f64::consts::PI;

/// Each ice band's shoreline wanders this far, in map heights.
const ICE_SHORE_NOISE: f64 = 0.07;
const CONTINENT_TRIES: usize = 80;
/// Continents stay this far from the side and top/bottom edges, in map heights.
const CONTINENT_MARGIN_X: f64 = 0.1;
const CONTINENT_MARGIN_Y: f64 = 0.08;
/// Continents gravitate towards this point, slightly north of centre.
const CONTINENT_CENTRE_Y: f64 = 0.45;
const ISLAND_TRIES: usize = 120;
/// Islands prefer about this many cells offshore, plus a bit per cell of radius.
const ISLAND_OFFSHORE_CELLS: f64 = 5.0;
/// Spacing (in cells) beyond which islands no longer repel each other.
const ISLAND_SPACING_CELLS: f64 = 25.0;
const FRAY_PASSES: u32 = 4;
const MAX_ISLE_SIZE: usize = 6;
const MAX_ISLE_WANDER: usize = 5;
const MAX_ISLE_TRIES: usize = 50_000;
/// Land within this distance of the map edge (in map heights) gets ever costlier to grow into.
const EDGE_ZONE: f64 = 0.09;
const EDGE_PENALTY: f64 = 400.0;

/// Assigns every world cell to a top-level region; cells left over become sea.
pub fn assign(geo: &Geo, mesh: &Mesh, perlin: &Perlin) -> Vec<Option<RegionId>> {
    let mut rng = Rng::new(hash_str(&geo.root));
    let offset = rng.f64() * 100.0;
    let on_map = mesh
        .pts
        .iter()
        .filter(|q| (0.0..WORLD_W).contains(&q[0]) && (0.0..WORLD_H).contains(&q[1]))
        .count() as f64;
    let mut world = WorldLayout {
        geo,
        mesh,
        perlin,
        offset,
        rng,
        owners: vec![None; mesh.len()],
        on_map,
    };
    let kids = &geo.region(RegionId::WORLD).children;
    let of_kind = |kind: RegionKind| kids.iter().copied().filter(move |&c| geo.region(c).kind == kind);

    let mut ice_height = 0.0;
    for id in of_kind(RegionKind::Polar) {
        ice_height = world.freeze_poles(id);
    }
    world.grow_continents(of_kind(RegionKind::Continent), ice_height);
    world.fray();
    let distance_to_land = mesh.bfs(|c| world.owners[c].is_some(), |c| world.owners[c].is_none());
    world.raise_islands(of_kind(RegionKind::Island), &distance_to_land);
    if let Some(id) = of_kind(RegionKind::Isles).next() {
        world.scatter_isles(id, &distance_to_land);
    }
    if let Some(id) = of_kind(RegionKind::Ocean).next() {
        world.owners.iter_mut().filter(|o| o.is_none()).for_each(|o| *o = Some(id));
    }
    world.owners
}

struct WorldLayout<'a> {
    geo: &'a Geo,
    mesh: &'a Mesh,
    perlin: &'a Perlin,
    offset: f64,
    rng: Rng,
    owners: Vec<Option<RegionId>>,
    /// Cells within the map edges, which the whole capacity is spread over.
    on_map: f64,
}

impl WorldLayout<'_> {
    fn budget(&self, id: RegionId) -> f64 {
        self.geo.region(id).size as f64 / self.geo.capacity.max(1) as f64 * self.on_map
    }

    /// Radius of a round landmass of `budget` cells, in map units.
    fn radius(&self, budget: f64) -> f64 {
        (budget / self.on_map * WORLD_W * WORLD_H / PI).sqrt()
    }

    /// System files become two Mercator ice bands along the north and south edges, with a noisy shoreline.
    /// Returns the bands' mean height.
    fn freeze_poles(&mut self, id: RegionId) -> f64 {
        let half = (self.budget(id) / 2.0).round() as usize;
        for (band, north) in [true, false].into_iter().enumerate() {
            let mut cells: Vec<(f64, usize)> = (0..self.mesh.len())
                .filter(|&c| self.owners[c].is_none())
                .map(|c| {
                    let [x, y] = self.mesh.pts[c];
                    let from_pole = if north { y } else { WORLD_H - y };
                    let shore = fbm01(self.perlin, x * 6.0 + self.offset, y * 6.0 + 40.0 * band as f64, 4) - 0.5;
                    (from_pole + ICE_SHORE_NOISE * shore, c)
                })
                .collect();
            cells.sort_by(|a, b| a.0.total_cmp(&b.0));
            cells.iter().take(half).for_each(|&(_, c)| self.owners[c] = Some(id));
        }
        half as f64 / self.on_map * WORLD_H
    }

    /// Places continents apart from each other and the ice, near the centre, and grows them along wandering spines.
    fn grow_continents(&mut self, ids: impl Iterator<Item = RegionId>, ice_height: f64) {
        let mut placed: Vec<(Pt, f64)> = Vec::new();
        let mut seeds = Vec::new();
        for id in ids {
            let budget = self.budget(id);
            let radius = self.radius(budget);
            let mut best = None;
            let mut best_score = f64::MIN;
            for _ in 0..CONTINENT_TRIES {
                let mx = (CONTINENT_MARGIN_X + radius).min(WORLD_W / 2.0);
                let my = (CONTINENT_MARGIN_Y + ice_height + radius).min(WORLD_H / 2.0);
                let x = mx + self.rng.f64() * (WORLD_W - 2.0 * mx);
                let y = my + self.rng.f64() * (WORLD_H - 2.0 * my).max(0.0);
                let Some(c) = self.mesh.nearest([x, y]).filter(|&c| self.owners[c].is_none()) else {
                    continue;
                };
                let gap = placed
                    .iter()
                    .map(|(q, qr)| (q[0] - x).hypot(q[1] - y) - qr - radius)
                    .fold(1.0, f64::min);
                let off_centre = (x - WORLD_W / 2.0).hypot(y - CONTINENT_CENTRE_Y);
                let score = gap - 0.5 * off_centre * radius;
                if score > best_score {
                    best_score = score;
                    best = Some(c);
                }
            }
            if let Some(c) = best {
                let at = self.mesh.pts[c];
                placed.push((at, radius));
                seeds.push(Seed {
                    region: id,
                    starts: vec![c],
                    budget,
                    spine: Some(Spine::new(at, radius, &mut self.rng)),
                });
            }
        }
        let claimed = self.grow(&seeds, true);
        // Nearly full disks can't fit separated continents: let the deficit grow into neighbours.
        let retry: Vec<Seed> = seeds
            .iter()
            .zip(&claimed)
            .filter(|(s, got)| (**got as f64) < s.budget - 1.0)
            .map(|(s, &got)| Seed {
                region: s.region,
                starts: frontier(self.mesh, &self.owners, s.region, None),
                budget: s.budget - got as f64,
                spine: None,
            })
            .collect();
        if !retry.is_empty() {
            self.grow(&retry, false);
        }
    }

    fn grow(&mut self, seeds: &[Seed], separate: bool) -> Vec<usize> {
        let (mesh, perlin, offset) = (self.mesh, self.perlin, self.offset);
        grow(mesh, &mut self.owners, seeds, None, separate, |c| {
            coast_cost(perlin, offset, mesh.pts[c])
        })
    }

    /// Coastline greebling: carves fjords and grows capes where coast noise is strong, trading cells one for one so
    /// every landmass keeps its exact area. Never lets two landmasses touch.
    fn fray(&mut self) {
        let (geo, mesh, perlin, offset) = (self.geo, self.mesh, self.perlin, self.offset);
        let owners = &mut self.owners;
        let is_land = |r: Option<RegionId>| r.is_some_and(|r| matches!(geo.region(r).kind, RegionKind::Continent | RegionKind::Island));
        for pass in 0..FRAY_PASSES {
            let frequency = 11.0 + f64::from(pass) * 6.0;
            let noise = |c: usize| {
                perlin.get([
                    mesh.pts[c][0] * frequency + offset * 3.0,
                    mesh.pts[c][1] * frequency - f64::from(pass),
                ])
            };
            let mut balance: HashMap<RegionId, i64> = HashMap::new();
            let coast: Vec<usize> = (0..mesh.len())
                .filter(|&c| is_land(owners[c]) && mesh.any_neighbour(c, |q| owners[q].is_none()))
                .collect();
            for c in coast {
                let rough = roughness(perlin, offset, mesh.pts[c][0], mesh.pts[c][1]);
                let Some(owner) = owners[c] else { continue };
                if noise(c) < -0.15 - 0.25 * (1.0 - rough)
                    && mesh.nbrs[c].iter().filter(|&&q| owners[q as usize] == Some(owner)).count() >= 2
                {
                    *balance.entry(owner).or_default() += 1;
                    owners[c] = None;
                }
            }
            let mut shore: Vec<(f64, usize)> = (0..mesh.len())
                .filter(|&c| owners[c].is_none() && mesh.any_neighbour(c, |q| is_land(owners[q])))
                .map(|c| (-noise(c), c))
                .collect();
            shore.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, c) in shore {
                let mut neighbours = mesh.nbrs[c].iter().filter_map(|&q| owners[q as usize]);
                let Some(owner) = neighbours.next() else { continue };
                if neighbours.any(|o| o != owner) || !is_land(Some(owner)) {
                    continue;
                }
                if let Some(debt) = balance.get_mut(&owner).filter(|debt| **debt > 0) {
                    owners[c] = Some(owner);
                    *debt -= 1;
                }
            }
        }
    }

    /// Drops islands offshore, spread apart, each grown along its own spine.
    fn raise_islands(&mut self, ids: impl Iterator<Item = RegionId>, distance_to_land: &[u32]) {
        let mut placed: Vec<Pt> = Vec::new();
        for id in ids {
            let budget = self.budget(id).max(1.0);
            let want = ISLAND_OFFSHORE_CELLS + 1.5 * (budget / PI).sqrt();
            let mut best = None;
            let mut best_score = f64::MIN;
            for _ in 0..ISLAND_TRIES {
                let c = self.rng.below(self.mesh.len());
                if self.owners[c].is_some() || distance_to_land[c] == UNREACHED {
                    continue;
                }
                let [x, y] = self.mesh.pts[c];
                let spacing = placed
                    .iter()
                    .map(|q| (q[0] - x).hypot(q[1] - y) / self.mesh.spacing)
                    .fold(ISLAND_SPACING_CELLS, f64::min);
                let score = -(f64::from(distance_to_land[c]) - want).abs() + 0.4 * spacing;
                if score > best_score {
                    best_score = score;
                    best = Some(c);
                }
            }
            if let Some(c) = best {
                let at = self.mesh.pts[c];
                placed.push(at);
                let spine = Spine::new(at, self.radius(budget), &mut self.rng);
                self.grow(&[Seed { region: id, starts: vec![c], budget, spine: Some(spine) }], true);
            }
        }
    }

    /// Dust becomes coastline: tiny islets hugging and fraying the shores.
    fn scatter_isles(&mut self, id: RegionId, distance_to_land: &[u32]) {
        let mut left = self.budget(id).round() as i64;
        let (geo, mesh) = (self.geo, self.mesh);
        let owners = &mut self.owners;
        let rng = &mut self.rng;
        let coast: Vec<usize> = (0..mesh.len())
            .filter(|&c| {
                owners[c].is_none() && mesh.any_neighbour(c, |q| owners[q].is_some_and(|r| geo.region(r).kind != RegionKind::Polar))
            })
            .collect();
        let mut tries = 0;
        while left > 0 && !coast.is_empty() && tries < MAX_ISLE_TRIES {
            tries += 1;
            let mut c = coast[rng.below(coast.len())];
            // Wander a few steps out to sea.
            for _ in 0..rng.below(MAX_ISLE_WANDER) {
                let further: Vec<usize> = mesh.nbrs[c]
                    .iter()
                    .map(|&q| q as usize)
                    .filter(|&q| owners[q].is_none() && distance_to_land[q] >= distance_to_land[c])
                    .collect();
                if further.is_empty() {
                    break;
                }
                c = further[rng.below(further.len())];
            }
            if owners[c].is_some() {
                continue;
            }
            let size = 1 + rng.below((left as usize).min(MAX_ISLE_SIZE));
            let mut queue = VecDeque::from([c]);
            let mut grown = 0;
            while let Some(x) = queue.pop_front() {
                if grown >= size {
                    break;
                }
                if owners[x].is_some() {
                    continue;
                }
                owners[x] = Some(id);
                grown += 1;
                queue.extend(mesh.nbrs[x].iter().map(|&y| y as usize).filter(|&y| owners[y].is_none()));
            }
            left -= grown as i64;
        }
    }
}

/// Growth cost for landmasses. Smooth zones grow round, Africa-like coasts; rough zones follow warped arcs
/// (peninsulas) with twigs branching only off those arcs (peninsulas of peninsulas), like Europe or Arabia.
fn coast_cost(perlin: &Perlin, offset: f64, [x, y]: Pt) -> f64 {
    let to_edge = x.min(WORLD_W - x).min(y).min(WORLD_H - y);
    let rough = roughness(perlin, offset, x, y);
    let wx = x + 0.09 * perlin.get([x * 2.3 + offset, y * 2.3]);
    let wy = y + 0.09 * perlin.get([x * 2.3, y * 2.3 - offset]);
    let arc = 1.0 - perlin.get([wx * 3.2 + offset, wy * 3.2]).abs();
    let twig = (1.0 - perlin.get([wx * 9.0 - offset, wy * 9.0 + 3.0]).abs()) * smoothstep(0.55, 0.85, arc);
    let ridge = arc.max(0.9 * twig);
    let jagged = 0.06 + 9.0 * (1.0 - ridge).powi(3);
    let round = 0.5 + 1.6 * fbm01(perlin, x * 9.0 - offset, y * 9.0 + 5.0, 3).powi(2);
    let lattice = 0.5 * fbm01(perlin, x * 45.0 - offset, y * 45.0, 2);
    (round + (jagged - round) * rough + lattice) * (1.0 + EDGE_PENALTY * (EDGE_ZONE - to_edge).max(0.0))
}
