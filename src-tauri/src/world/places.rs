//! Where names go: a label anchor per region and depth, and a city per settled region.
use super::Owners;
use super::mesh::Mesh;
use super::terrain::regions_with_children;
use crate::geo::{Geo, RegionId, RegionKind};
use rayon::prelude::*;
use serde::{Serialize, Serializer};
use std::collections::{HashMap, HashSet};

pub struct Label {
    id: RegionId,
    /// Pole of inaccessibility: the cell furthest from the region's border.
    anchor: [f64; 2],
    /// Distance from the anchor to the border.
    radius: f64,
    /// Hops from the anchor to the border, to find the best anchor.
    anchor_hops: u32,
    bbox: [f64; 4],
    cells: u32,
}

impl Serialize for Label {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let [x0, y0, x1, y1] = self.bbox;
        (self.id, self.anchor[0], self.anchor[1], self.radius, x0, y0, x1, y1, self.cells).serialize(s)
    }
}

pub struct City {
    id: RegionId,
    at: [f64; 2],
    population: u64,
}

impl Serialize for City {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        (self.id, self.at[0], self.at[1], self.population).serialize(s)
    }
}

/// Labels for every region drawn here, or with `placed` only for those regions, at every depth: a continent's
/// cells also anchor its nations' labels.
pub fn labels(geo: &Geo, mesh: &Mesh, owners: &Owners, placed: Option<&HashSet<RegionId>>) -> Vec<Label> {
    let wanted = |r: RegionId| placed.is_none_or(|p| p.contains(&r)) && geo.region(r).kind != RegionKind::Isles;
    let max_depth = owners.iter().flatten().map(|&r| geo.region(r).depth).max().unwrap_or(0);
    let layers: Vec<(Vec<Option<RegionId>>, Vec<u32>)> = (1..=max_depth)
        .into_par_iter()
        .map(|depth| {
            let at_depth: Vec<Option<RegionId>> = owners
                .iter()
                .map(|&r| r.filter(|&r| geo.region(r).depth >= depth).map(|r| geo.ancestor_at(r, depth)))
                .collect();
            let is_border = |c: usize| mesh.edge[c] || mesh.any_neighbour(c, |q| at_depth[q] != at_depth[c]);
            let to_border = mesh.bfs(|c| at_depth[c].is_some() && is_border(c), |c| at_depth[c].is_some());
            (at_depth, to_border)
        })
        .collect();
    let mut labels: HashMap<RegionId, Label> = HashMap::new();
    for (at_depth, to_border) in layers {
        for c in 0..mesh.len() {
            let Some(id) = at_depth[c].filter(|&r| wanted(r)) else { continue };
            let [x, y] = mesh.pts[c];
            let label = labels.entry(id).or_insert(Label {
                id,
                anchor: [x, y],
                radius: 0.0,
                anchor_hops: 0,
                bbox: [x, y, x, y],
                cells: 0,
            });
            label.cells += 1;
            label.bbox = [
                label.bbox[0].min(x),
                label.bbox[1].min(y),
                label.bbox[2].max(x),
                label.bbox[3].max(y),
            ];
            if to_border[c] >= label.anchor_hops {
                label.anchor_hops = to_border[c];
                label.anchor = [x, y];
                label.radius = (f64::from(to_border[c]) + 0.5) * mesh.spacing;
            }
        }
    }
    let mut labels: Vec<Label> = labels.into_values().collect();
    labels.sort_by_key(|l| l.id);
    labels
}

/// A city at the heart of each region with one, drawn here (`placed`, or everywhere without it) or the patch's own
/// `root`, sized by the files not already shown as child regions.
pub fn cities(geo: &Geo, mesh: &Mesh, owners: &Owners, placed: Option<&HashSet<RegionId>>, root: RegionId) -> Vec<City> {
    let wanted = |r: RegionId| placed.is_none_or(|p| p.contains(&r)) || r == root;
    let parents = regions_with_children(geo, owners);
    let to_border = mesh.bfs(
        |c| owners[c].is_some() && (mesh.edge[c] || mesh.any_neighbour(c, |q| owners[q] != owners[c])),
        |c| owners[c].is_some(),
    );
    let mut hearts: HashMap<RegionId, (u32, usize)> = HashMap::new();
    for c in 0..mesh.len() {
        let Some(r) = owners[c] else { continue };
        if !geo.region(r).settled || !wanted(r) {
            continue;
        }
        let heart = hearts.entry(r).or_insert((0, c));
        if to_border[c] > heart.0 {
            *heart = (to_border[c], c);
        }
    }
    let mut cities: Vec<City> = hearts
        .into_iter()
        .map(|(id, (_, c))| {
            let reg = geo.region(id);
            let population = if parents.contains(&id) { reg.dust.count } else { reg.count };
            City { id, at: mesh.pts[c], population }
        })
        .filter(|c| c.population > 0)
        .collect();
    cities.sort_by_key(|c| c.id);
    cities
}
