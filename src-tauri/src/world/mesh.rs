//! A jittered hex lattice of points and its Delaunay triangulation: the cells everything else is painted on.
use crate::rng::{Rng, hash_str};
use delaunator::{Point, triangulate};
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::VecDeque;

/// Each point moves up to this fraction of the lattice spacing off its hex position.
const JITTER: f64 = 0.45;
/// Triangles with an edge longer than this many lattice spacings span a gap, not land.
const MAX_EDGE_SPACINGS: f64 = 3.0;
/// The nearest point to any position is within this many lattice rows and columns of its grid square.
const NEAREST_REACH: i64 = 2;
/// BFS distance of cells that can't be reached.
pub const UNREACHED: u32 = u32::MAX;

pub type Pt = [f64; 2];

pub struct Mesh {
    pub origin: Pt,
    /// Lattice spacing: roughly the distance between neighbouring points.
    pub spacing: f64,
    pub cols: usize,
    pub rows: usize,
    pub pts: Vec<Pt>,
    /// Points the patch covers; the rest pad the lattice so the triangulation stays regular.
    pub inside: Vec<bool>,
    /// Inside points on the boundary of the covered area.
    pub edge: Vec<bool>,
    /// Neighbours through kept triangles, sorted.
    pub nbrs: Vec<Vec<u32>>,
    /// Kept triangles (all corners inside, no long edges), three point indices each.
    pub tris: Vec<u32>,
}

/// The full triangulation, whose halfedges pair up adjacent triangles for tracing Voronoi borders.
pub struct Dual {
    pub triangles: Vec<usize>,
    pub halfedges: Vec<usize>,
    /// Per triangle: whether it was kept in [`Mesh::tris`].
    pub kept: Vec<bool>,
}

impl Mesh {
    /// Covers `size` from `origin` with about `cells` points, keeping those for which `locate` finds something and
    /// returning what it found per point.
    pub fn build<T: Send>(
        seed: &str,
        origin: Pt,
        size: Pt,
        cells: usize,
        locate: impl Fn(Pt) -> Option<T> + Sync,
    ) -> (Mesh, Dual, Vec<Option<T>>) {
        let [w, h] = size;
        let spacing = (w * h / cells as f64).sqrt();
        let cols = (w / spacing).ceil() as usize;
        let rows = (h / spacing).ceil() as usize;
        let mut rng = Rng::new(hash_str(seed));
        let mut pts = Vec::with_capacity(cols * rows);
        for j in 0..rows {
            for i in 0..cols {
                let hex = if j % 2 == 1 { 0.25 } else { -0.25 };
                let x = origin[0] + (i as f64 + 0.5 + hex + (rng.f64() - 0.5) * JITTER) * spacing;
                let y = origin[1] + (j as f64 + 0.5 + (rng.f64() - 0.5) * JITTER) * spacing;
                pts.push([x, y]);
            }
        }
        let located: Vec<Option<T>> = pts.par_iter().map(|&p| locate(p)).collect();
        let inside: Vec<bool> = located.iter().map(Option::is_some).collect();
        let delaunay = triangulate(&pts.iter().map(|p| Point { x: p[0], y: p[1] }).collect::<Vec<_>>());

        let mut nbrs: Vec<Vec<u32>> = (0..pts.len()).map(|_| Vec::with_capacity(12)).collect();
        let mut edge = vec![false; pts.len()];
        let mut tris = Vec::new();
        let mut kept = Vec::with_capacity(delaunay.triangles.len() / 3);
        for tri in delaunay.triangles.chunks(3) {
            let spans_gap = (0..3).any(|k| {
                let (a, b) = (pts[tri[k]], pts[tri[(k + 1) % 3]]);
                (a[0] - b[0]).hypot(a[1] - b[1]) > MAX_EDGE_SPACINGS * spacing
            });
            let keep = !spans_gap && tri.iter().all(|&v| inside[v]);
            kept.push(keep);
            if keep {
                tris.extend(tri.iter().map(|&v| v as u32));
                for k in 0..3 {
                    let (a, b) = (tri[k], tri[(k + 1) % 3]);
                    nbrs[a].push(b as u32);
                    nbrs[b].push(a as u32);
                }
            } else {
                for &v in tri {
                    edge[v] |= inside[v];
                }
            }
        }
        delaunay.hull.iter().for_each(|&v| edge[v] = true);
        nbrs.par_iter_mut().for_each(|v| {
            v.sort_unstable();
            v.dedup();
            v.shrink_to_fit();
        });
        let mesh = Mesh { origin, spacing, cols, rows, pts, inside, edge, nbrs, tris };
        (
            mesh,
            Dual {
                triangles: delaunay.triangles,
                halfedges: delaunay.halfedges,
                kept,
            },
            located,
        )
    }

    pub fn len(&self) -> usize {
        self.pts.len()
    }

    /// `[x0, y0, x1, y1]` of the lattice.
    pub fn bounds(&self) -> [f64; 4] {
        let [x, y] = self.origin;
        [x, y, x + self.cols as f64 * self.spacing, y + self.rows as f64 * self.spacing]
    }

    pub fn nearest(&self, [x, y]: Pt) -> Option<usize> {
        let col = ((x - self.origin[0]) / self.spacing).floor() as i64;
        let row = ((y - self.origin[1]) / self.spacing).floor() as i64;
        let mut best = None;
        let mut best_d2 = f64::MAX;
        for j in (row - NEAREST_REACH).max(0)..=(row + NEAREST_REACH).min(self.rows as i64 - 1) {
            for i in (col - NEAREST_REACH).max(0)..=(col + NEAREST_REACH).min(self.cols as i64 - 1) {
                let k = j as usize * self.cols + i as usize;
                let d2 = dist2(self.pts[k], [x, y]);
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = Some(k);
                }
            }
        }
        best
    }

    fn neighbours(&self, c: usize) -> impl Iterator<Item = usize> + '_ {
        self.nbrs[c].iter().map(|&q| q as usize)
    }

    pub fn any_neighbour(&self, c: usize, pred: impl FnMut(usize) -> bool) -> bool {
        self.neighbours(c).any(pred)
    }

    /// Hop distance from the inside cells matching `source`, spreading only into cells that `pass`.
    pub fn bfs(&self, source: impl Fn(usize) -> bool, pass: impl Fn(usize) -> bool) -> Vec<u32> {
        let mut dist = vec![UNREACHED; self.len()];
        let mut queue = VecDeque::new();
        for c in (0..self.len()).filter(|&c| self.inside[c] && source(c)) {
            dist[c] = 0;
            queue.push_back(c);
        }
        while let Some(c) = queue.pop_front() {
            for n in self.neighbours(c) {
                if dist[n] == UNREACHED && pass(n) {
                    dist[n] = dist[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        dist
    }

    /// Blends each masked cell halfway towards the mean of its masked neighbours, `iterations` times.
    pub fn diffuse(&self, values: &mut [f32], mask: &[bool], iterations: usize) {
        for _ in 0..iterations {
            let prev = values.to_vec();
            values.par_iter_mut().enumerate().filter(|(c, _)| mask[*c]).for_each(|(c, value)| {
                let (sum, count) = self.nbrs[c]
                    .iter()
                    .filter(|&&q| mask[q as usize])
                    .fold((0.0, 0), |(s, k), &q| (s + prev[q as usize], k + 1));
                if count > 0 {
                    *value = 0.5 * prev[c] + 0.5 * sum / count as f32;
                }
            });
        }
    }
}

pub fn dist2(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

pub fn circumcenter(a: Pt, b: Pt, c: Pt) -> Pt {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-18 {
        return [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    [
        (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d,
        (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d,
    ]
}

/// Min-heap entry ordered by cost alone. Ties pop in `BinaryHeap`'s insertion-dependent order, which the generated
/// maps rely on, so don't add tie-breakers.
pub struct Cheapest<T> {
    pub cost: f64,
    pub item: T,
}

impl<T> PartialEq for Cheapest<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<T> Eq for Cheapest<T> {}

impl<T> PartialOrd for Cheapest<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Cheapest<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}
