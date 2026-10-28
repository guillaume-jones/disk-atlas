//! Map generation. The world is one Voronoi mesh; zooming in requests finer region patches (the next levels of the
//! region tree, inheriting the parent's layout) and quadtree terrain tiles (more terrain detail only).
mod continents;
mod encode;
mod grow;
mod layout;
mod lines;
mod mesh;
mod noise;
mod places;
mod terrain;

use crate::geo::{Geo, RegionId, RegionKind};
use crate::rng::hash_str;
use ::noise::Perlin;
use layout::Layout;
use lines::Segment;
use mesh::{Mesh, Pt, dist2};
use places::{City, Label};
use rayon::prelude::*;
use terrain::Terrain;

/// Map extent in map units; tiles subdivide the unit square, so a level-0 tile is one map height.
const WORLD_W: f64 = 2.0;
const WORLD_H: f64 = 1.0;
/// Points per mesh: the world, a region patch at its densest, and a tile.
const TARGET_CELLS: usize = 100_000;
/// Cap for patches over sparse, sprawling regions, whose bounding box is mostly empty.
const MAX_PATCH_POINTS: usize = 350_000;
const MIN_PATCH_FILL: f64 = 0.02;
/// Meshes extend this many parent cells past what they cover, so the outermost cells are complete.
const PAD_SPACINGS: f64 = 1.5;
/// A tile point only inherits land from a parent cell this close (in parent spacings).
const LAND_REACH_SPACINGS: f64 = 1.5;
pub const MAX_TILE_LEVEL: u32 = 24;

/// Region owning each cell; `None` outside the patch.
type Owners = [Option<RegionId>];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub level: u32,
    pub x: i64,
    pub y: i64,
}

impl TileKey {
    fn size(self) -> f64 {
        0.5f64.powi(self.level as i32)
    }

    fn origin(self) -> Pt {
        [self.x as f64 * self.size(), self.y as f64 * self.size()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Region(RegionId),
    Tile { level: u32 },
}

/// Where a point of a finer mesh falls in its parent patches.
#[derive(Clone, Copy)]
struct Located {
    parent: usize,
    cell: usize,
}

/// A generated mesh with its terrain, kept as the parent of finer patches and tiles.
pub struct Patch {
    scope: Scope,
    mesh: Mesh,
    owners: Vec<Option<RegionId>>,
    ice: Vec<bool>,
    terrain: Terrain,
}

impl Patch {
    pub fn spacing(&self) -> f64 {
        self.mesh.spacing
    }

    pub fn overlaps(&self, tile: TileKey) -> bool {
        let [x0, y0, x1, y1] = self.mesh.bounds();
        let ([tx, ty], size) = (tile.origin(), tile.size());
        x0 < tx + size && x1 > tx && y0 < ty + size && y1 > ty
    }

    /// The land cell owning `pt`, if this patch has one there.
    fn land_at(&self, geo: &Geo, pt: Pt) -> Option<usize> {
        let reach = LAND_REACH_SPACINGS * self.mesh.spacing;
        self.mesh.nearest(pt).filter(|&q| {
            self.mesh.inside[q]
                && self.owners[q].is_some_and(|r| geo.region(r).kind != RegionKind::Ocean)
                && dist2(self.mesh.pts[q], pt) < reach.powi(2)
        })
    }
}

/// What's drawn over the terrain; only needed for the payload.
#[derive(Default)]
struct Overlay {
    shade: Vec<f32>,
    borders: Vec<Segment>,
    rivers: Vec<Segment>,
    labels: Vec<Label>,
    cities: Vec<City>,
}

pub struct Built {
    pub patch: Patch,
    pub payload: Vec<u8>,
    /// One line of statistics for the CLI.
    pub summary: String,
}

fn perlin(geo: &Geo) -> Perlin {
    Perlin::new(hash_str(&geo.root) as u32)
}

pub fn build_world(geo: &Geo) -> serde_json::Result<Built> {
    let pad = PAD_SPACINGS * (WORLD_W * WORLD_H / TARGET_CELLS as f64).sqrt();
    let size = [WORLD_W + 2.0 * pad, WORLD_H + 2.0 * pad];
    let area = size[0] * size[1] / (WORLD_W * WORLD_H);
    let (mesh, dual, _) = Mesh::build("world", [-pad, -pad], size, (TARGET_CELLS as f64 * area) as usize, |_| Some(()));
    let perlin = perlin(geo);
    let mut owners = continents::assign(geo, &mesh, &perlin);
    let mut layout = Layout::new(geo, &mesh, &perlin, RegionId::WORLD);
    for &id in &geo.region(RegionId::WORLD).children {
        let cells: Vec<usize> = (0..mesh.len()).filter(|&c| owners[c] == Some(id)).collect();
        layout.subdivide(&mut owners, id, &cells);
    }
    let ice: Vec<bool> = owners
        .iter()
        .map(|r| r.is_some_and(|r| geo.top(r).kind == RegionKind::Polar))
        .collect();
    let terrain = terrain::world(geo, &mesh, &owners, &ice, &perlin);
    let overlay = Overlay {
        shade: terrain::shade(&mesh, &terrain.elevation),
        borders: lines::borders(geo, &mesh, &dual, &owners, None, &perlin),
        rivers: lines::rivers(&mesh, &terrain.elevation, &ice, &perlin),
        labels: places::labels(geo, &mesh, &owners, None),
        cities: places::cities(geo, &mesh, &owners, None, RegionId::WORLD),
    };
    let patch = Patch {
        scope: Scope::Region(RegionId::WORLD),
        mesh,
        owners,
        ice,
        terrain,
    };
    package(geo, patch, &overlay)
}

/// Finer patch for `region`, inheriting the parent's layout and adding the next levels of the region tree.
pub fn build_patch(geo: &Geo, parent: &Patch, region: RegionId) -> serde_json::Result<Built> {
    let pm = &parent.mesh;
    let within = |q: usize| pm.inside[q] && parent.owners[q].is_some_and(|r| geo.is_within(r, region));
    let cells: Vec<usize> = (0..pm.len()).filter(|&c| within(c)).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &c in &cells {
        let [x, y] = pm.pts[c];
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let pad = pm.spacing * PAD_SPACINGS;
    let size = [x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad];
    let fill = (cells.len() as f64 * pm.spacing * pm.spacing / (size[0] * size[1])).clamp(MIN_PATCH_FILL, 1.0);
    let points = ((TARGET_CELLS as f64 / fill) as usize).min(MAX_PATCH_POINTS);
    let locate = |pt: Pt| pm.nearest(pt).filter(|&q| within(q)).map(|cell| Located { parent: 0, cell });
    let (mesh, dual, located) = Mesh::build(&format!("region{region}"), [x0 - pad, y0 - pad], size, points, locate);
    let parents = [parent];
    let (mut owners, ice) = inherit(&parents, &located);
    let perlin = perlin(geo);
    let mut layout = Layout::new(geo, &mesh, &perlin, region);
    let all: Vec<usize> = (0..mesh.len()).filter(|&c| mesh.inside[c]).collect();
    layout.subdivide(&mut owners, region, &all);
    let placed = layout.placed;
    let terrain = terrain::refine(geo, &mesh, &owners, &ice, &parents, &located, &perlin);
    let overlay = Overlay {
        shade: terrain::shade(&mesh, &terrain.elevation),
        borders: lines::borders(geo, &mesh, &dual, &owners, Some(&placed), &perlin),
        rivers: Vec::new(),
        labels: places::labels(geo, &mesh, &owners, Some(&placed)),
        cities: places::cities(geo, &mesh, &owners, Some(&placed), region),
    };
    let patch = Patch { scope: Scope::Region(region), mesh, owners, ice, terrain };
    package(geo, patch, &overlay)
}

/// Square quadtree tile adding terrain detail beyond what the region tree provides. Each point inherits from the
/// finest of `parents` (sorted fine to coarse) that has land there.
pub fn build_tile(geo: &Geo, parents: &[&Patch], tile: TileKey) -> serde_json::Result<Built> {
    let locate = |pt: Pt| {
        parents
            .iter()
            .enumerate()
            .find_map(|(parent, p)| p.land_at(geo, pt).map(|cell| Located { parent, cell }))
    };
    let seed = format!("tile{}/{}/{}", tile.level, tile.x, tile.y);
    let (mesh, _, located) = Mesh::build(&seed, tile.origin(), [tile.size(), tile.size()], TARGET_CELLS, locate);
    let (owners, ice) = inherit(parents, &located);
    let terrain = terrain::refine(geo, &mesh, &owners, &ice, parents, &located, &perlin(geo));
    let overlay = Overlay {
        shade: terrain::shade(&mesh, &terrain.elevation),
        ..Overlay::default()
    };
    let patch = Patch {
        scope: Scope::Tile { level: tile.level },
        mesh,
        owners,
        ice,
        terrain,
    };
    package(geo, patch, &overlay)
}

fn inherit(parents: &[&Patch], located: &[Option<Located>]) -> (Vec<Option<RegionId>>, Vec<bool>) {
    located
        .par_iter()
        .map(|l| {
            l.map_or((None, false), |Located { parent, cell }| {
                (parents[parent].owners[cell], parents[parent].ice[cell])
            })
        })
        .unzip()
}

fn package(geo: &Geo, patch: Patch, overlay: &Overlay) -> serde_json::Result<Built> {
    let payload = encode::encode(geo, &patch, overlay)?;
    let summary = format!(
        "{} points, {} triangles, {} border segs, {} river segs, {} labels, {} cities ({:.1} MB)",
        patch.mesh.inside.iter().filter(|&&i| i).count(),
        patch.mesh.tris.len() / 3,
        overlay.borders.len(),
        overlay.rivers.len(),
        overlay.labels.len(),
        overlay.cities.len(),
        payload.len() as f64 / 1e6
    );
    Ok(Built { patch, payload, summary })
}
