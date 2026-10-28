use crate::geo::{Geo, RKind, WORLD};
use crate::util::{NONE, Rng, hash_str, smoothstep};
use delaunator::{EMPTY, Point, triangulate};
use noise::{NoiseFn, Perlin};
use rayon::prelude::*;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::f64::consts::PI;

pub const WORLD_W: f64 = 2.0;
pub const WORLD_H: f64 = 1.0;
const CELLS: usize = 100_000;
const MAX_POINTS: usize = 350_000;
/// Regions that would get fewer cells than this wait for a finer patch.
const MIN_CELLS: f64 = 6.0;
const TAU_DAYS: f64 = 60.0;
/// Slow tail so data older than a year still separates (1 year vs 2+ years).
const TAIL_DAYS: f64 = 400.0;
const BASE_FREQ: f64 = 3.0;

pub struct Label {
    id: u32,
    x: f64,
    y: f64,
    r: f64,
    bbox: [f64; 4],
    cells: u32,
}

pub struct City {
    id: u32,
    x: f64,
    y: f64,
    pop: u64,
}

/// A Voronoi mesh covering either the whole world or one region at higher resolution.
pub struct Patch {
    pub root: u32,
    /// Quadtree level for detail tiles; region patches have none.
    pub level: Option<u32>,
    ox: f64,
    oy: f64,
    s: f64,
    cols: usize,
    rows: usize,
    octaves: u32,
    pts: Vec<[f64; 2]>,
    inside: Vec<bool>,
    edge: Vec<bool>,
    tris: Vec<u32>,
    good: Vec<bool>,
    triangles: Vec<usize>,
    halfedges: Vec<usize>,
    nbrs: Vec<Vec<u32>>,
    cell: Vec<u32>,
    rec: Vec<f32>,
    elev: Vec<f32>,
    ice: Vec<bool>,
    shade: Vec<f32>,
    placed: HashSet<u32>,
    borders: Vec<f32>,
    rivers: Vec<f32>,
    labels: Vec<Label>,
    cities: Vec<City>,
}

impl Patch {
    fn nearest(&self, x: f64, y: f64) -> Option<usize> {
        let fi = ((x - self.ox) / self.s).floor() as i64;
        let fj = ((y - self.oy) / self.s).floor() as i64;
        let mut best = None;
        let mut bd = f64::MAX;
        for j in (fj - 2).max(0)..=(fj + 2).min(self.rows as i64 - 1) {
            for i in (fi - 2).max(0)..=(fi + 2).min(self.cols as i64 - 1) {
                let k = j as usize * self.cols + i as usize;
                let d = (self.pts[k][0] - x).powi(2) + (self.pts[k][1] - y).powi(2);
                if d < bd {
                    bd = d;
                    best = Some(k);
                }
            }
        }
        best
    }

    fn len(&self) -> usize {
        self.pts.len()
    }

    /// The land cell owning `pt`, if this patch has one there.
    fn land_at(&self, geo: &Geo, pt: [f64; 2]) -> Option<usize> {
        self.nearest(pt[0], pt[1]).filter(|&q| {
            let d2 = (self.pts[q][0] - pt[0]).powi(2) + (self.pts[q][1] - pt[1]).powi(2);
            self.inside[q] && self.cell[q] != NONE && geo.regions[self.cell[q] as usize].kind != RKind::Ocean && d2 < (1.5 * self.s).powi(2)
        })
    }

    pub fn bounds(&self) -> [f64; 4] {
        [self.ox, self.oy, self.ox + self.cols as f64 * self.s, self.oy + self.rows as f64 * self.s]
    }

    pub fn spacing(&self) -> f64 {
        self.s
    }

    /// Drops everything not needed to act as a parent for finer patches.
    pub fn compact(&mut self) {
        (self.triangles, self.halfedges) = (Vec::new(), Vec::new());
        (self.good, self.edge, self.shade) = (Vec::new(), Vec::new(), Vec::new());
        (self.borders, self.rivers) = (Vec::new(), Vec::new());
        (self.labels, self.cities, self.placed) = (Vec::new(), Vec::new(), HashSet::new());
    }
}

fn mesh(root: u32, seed: &str, ox: f64, oy: f64, w: f64, h: f64, n: usize, inside: impl Fn([f64; 2]) -> bool + Sync) -> Patch {
    let s = (w * h / n as f64).sqrt();
    let (cols, rows) = ((w / s).ceil() as usize, (h / s).ceil() as usize);
    let mut rng = Rng::new(hash_str(seed));
    let mut pts = Vec::with_capacity(cols * rows);
    for j in 0..rows {
        for i in 0..cols {
            let hex = if j % 2 == 1 { 0.25 } else { -0.25 };
            let x = ox + (i as f64 + 0.5 + hex + (rng.f64() - 0.5) * 0.45) * s;
            let y = oy + (j as f64 + 0.5 + (rng.f64() - 0.5) * 0.45) * s;
            pts.push([x, y]);
        }
    }
    let inside: Vec<bool> = pts.par_iter().map(|&p| inside(p)).collect();
    let t = triangulate(&pts.iter().map(|p| Point { x: p[0], y: p[1] }).collect::<Vec<_>>());
    let mut nbrs: Vec<Vec<u32>> = (0..pts.len()).map(|_| Vec::with_capacity(12)).collect();
    let mut edge = vec![false; pts.len()];
    let mut tris = Vec::new();
    let mut good = Vec::with_capacity(t.triangles.len() / 3);
    for tri in t.triangles.chunks(3) {
        let long = (0..3).any(|k| {
            let (a, b) = (pts[tri[k]], pts[tri[(k + 1) % 3]]);
            (a[0] - b[0]).hypot(a[1] - b[1]) > 3.0 * s
        });
        good.push(!long && tri.iter().all(|&v| inside[v]));
        if *good.last().unwrap() {
            tris.extend(tri.iter().map(|&v| v as u32));
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                nbrs[a].push(b as u32);
                nbrs[b].push(a as u32);
            }
        } else {
            tri.iter().for_each(|&v| edge[v] |= inside[v]);
        }
    }
    t.hull.iter().for_each(|&v| edge[v] = true);
    nbrs.par_iter_mut().for_each(|v| {
        v.sort_unstable();
        v.dedup();
        v.shrink_to_fit();
    });
    let len = pts.len();
    Patch {
        root,
        level: None,
        ox,
        oy,
        s,
        cols,
        rows,
        octaves: ((1.0 / BASE_FREQ) / (6.0 * s)).log2().ceil().max(1.0) as u32,
        pts,
        inside,
        edge,
        tris,
        good,
        triangles: t.triangles,
        halfedges: t.halfedges,
        nbrs,
        cell: vec![NONE; len],
        rec: vec![0.0; len],
        elev: vec![0.0; len],
        ice: vec![false; len],
        shade: vec![0.0; len],
        placed: HashSet::new(),
        borders: Vec::new(),
        rivers: Vec::new(),
        labels: Vec::new(),
        cities: Vec::new(),
    }
}

fn fbm01(n: &Perlin, x: f64, y: f64, oct: u32) -> f64 {
    let (mut s, mut a, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for _ in 0..oct {
        s += a * n.get([x * f, y * f]);
        norm += a;
        a *= 0.5;
        f *= 2.0;
    }
    (0.5 + 0.5 * s / norm).clamp(0.0, 1.0)
}

/// Ridged multifractal octaves `from..to`. Mountains scale with recency `r`, so stale data stays flat.
fn detail(n: &Perlin, x: f64, y: f64, from: u32, to: u32, r: f64) -> f64 {
    let wx = x + 0.06 * n.get([x * 2.1 + 40.0, y * 2.1]);
    let wy = y + 0.06 * n.get([x * 2.1, y * 2.1 - 40.0]);
    let mut s = 0.0;
    for i in from..to {
        let (f, a) = (BASE_FREQ * 2f64.powi(i as i32), 0.5f64.powi(i as i32));
        let ridge = (1.0 - n.get([wx * f + 11.3 * i as f64, wy * f - 7.1 * i as f64]).abs()).powi(2);
        let hill = n.get([x * f * 1.7 + 101.0, y * f * 1.7 - 33.0]);
        s += a * ((0.08 + 0.92 * r) * 0.5 * ridge + 0.1 * hill);
    }
    s
}

#[derive(PartialEq)]
struct Item(f64, u32, u32);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

struct Seed {
    region: u32,
    starts: Vec<usize>,
    budget: f64,
    spine: Option<Spine>,
}

/// A wandering backbone (a random walk of segments either side of the seed) that a landmass grows along.
#[derive(Clone)]
struct Spine {
    pts: Vec<[f64; 2]>,
    w: f64,
}

impl Spine {
    fn new(at: [f64; 2], radius: f64, rng: &mut Rng) -> Self {
        let theta = rng.f64() * 2.0 * PI;
        let step = radius * 0.55;
        let mut pts = vec![at];
        for dir in [0.0, PI] {
            let (mut p, mut a) = (at, theta + dir);
            for _ in 0..5 {
                a += (rng.f64() - 0.5) * 1.9;
                p = [p[0] + step * a.cos(), p[1] + step * a.sin()];
                if dir == 0.0 { pts.push(p) } else { pts.insert(0, p) }
            }
        }
        Spine { pts, w: radius * 0.55 }
    }

    fn factor(&self, q: [f64; 2]) -> f64 {
        let d = self
            .pts
            .windows(2)
            .map(|s| {
                let ([ax, ay], [bx, by]) = (s[0], s[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((q[0] - ax) * dx + (q[1] - ay) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0.0, 1.0);
                (q[0] - ax - t * dx).hypot(q[1] - ay - t * dy)
            })
            .fold(f64::MAX, f64::min)
            / self.w;
        1.0 + 4.0 * d * d
    }
}

/// Simultaneous region growing over cells still owned by `avail`, each seed claiming up to its cell budget.
/// Cost is normalised by sqrt(budget) so large regions expand proportionally faster, like a power diagram.
fn grow(p: &mut Patch, seeds: &[Seed], avail: u32, separate: bool, cost: &dyn Fn(&Patch, usize) -> f64) -> Vec<usize> {
    let mut heap = BinaryHeap::new();
    let mut got = vec![0usize; seeds.len()];
    for (k, sd) in seeds.iter().enumerate() {
        sd.starts.iter().for_each(|&c| heap.push(Item(0.0, c as u32, k as u32)));
    }
    while let Some(Item(d, c, k)) = heap.pop() {
        let (c, k) = (c as usize, k as usize);
        let sd = &seeds[k];
        if p.cell[c] != avail || got[k] as f64 >= sd.budget {
            continue;
        }
        if separate && p.nbrs[c].iter().any(|&q| p.cell[q as usize] != avail && p.cell[q as usize] != sd.region) {
            continue;
        }
        p.cell[c] = sd.region;
        got[k] += 1;
        let norm = sd.budget.sqrt().max(1.0);
        for i in 0..p.nbrs[c].len() {
            let q = p.nbrs[c][i] as usize;
            if p.cell[q] == avail {
                let shape = sd.spine.as_ref().map_or(1.0, |sp| sp.factor(p.pts[q]));
                heap.push(Item(d + cost(p, q) * shape / norm, q as u32, k as u32));
            }
        }
    }
    got
}

fn frontier(p: &Patch, region: u32, avail: u32) -> Vec<usize> {
    (0..p.len()).filter(|&c| p.cell[c] == avail && p.nbrs[c].iter().any(|&q| p.cell[q as usize] == region)).collect()
}

fn bfs(p: &Patch, source: impl Fn(usize) -> bool, pass: impl Fn(usize) -> bool) -> Vec<u32> {
    let mut d = vec![u32::MAX; p.len()];
    let mut q = VecDeque::new();
    for c in 0..p.len() {
        if p.inside[c] && source(c) {
            d[c] = 0;
            q.push_back(c);
        }
    }
    while let Some(c) = q.pop_front() {
        for &n in &p.nbrs[c] {
            let n = n as usize;
            if d[n] == u32::MAX && pass(n) {
                d[n] = d[c] + 1;
                q.push_back(n);
            }
        }
    }
    d
}

fn diffuse(p: &Patch, v: &mut [f32], mask: &[bool], iters: usize) {
    for _ in 0..iters {
        let prev = v.to_vec();
        v.par_iter_mut().enumerate().filter(|(c, _)| mask[*c]).for_each(|(c, x)| {
            let (s, k) = p.nbrs[c].iter().filter(|&&q| mask[q as usize]).fold((0.0, 0), |(s, k), &q| (s + prev[q as usize], k + 1));
            if k > 0 {
                *x = 0.5 * prev[c] + 0.5 * s / k as f32;
            }
        });
    }
}

/// Regions with at least one descendant holding cells in this patch.
fn with_kids(geo: &Geo, p: &Patch) -> HashSet<u32> {
    let mut set = HashSet::new();
    for &r in &p.cell {
        if r == NONE {
            continue;
        }
        let mut a = geo.regions[r as usize].parent;
        while a != NONE && set.insert(a) {
            a = geo.regions[a as usize].parent;
        }
    }
    set
}

/// exp(-age/τ) per cell: 1 for today, ~0 after a year. A region's own cells use its dust's age once children are drawn.
fn recency(geo: &Geo, p: &Patch) -> Vec<f32> {
    let kids = with_kids(geo, p);
    p.cell
        .iter()
        .map(|&r| {
            if r == NONE {
                return 0.0;
            }
            let reg = &geo.regions[r as usize];
            let mt = if kids.contains(&r) && reg.dust.weight > 0.0 { reg.dust.mtime() } else { reg.mtime };
            let age = ((geo.now - mt) / 86400.0).max(0.0);
            (0.85 * (-age / TAU_DAYS).exp() + 0.15 * (-age / TAIL_DAYS).exp()) as f32
        })
        .collect()
}

/// Lay out the children of `x` inside `cells` (x's subtree in this patch), then recurse.
/// Children already present (inherited from a coarser patch) keep their cells.
fn layout(geo: &Geo, p: &mut Patch, x: u32, cells: Vec<usize>, perlin: &Perlin) {
    let reg = &geo.regions[x as usize];
    if reg.children.is_empty() || cells.is_empty() || reg.size == 0 {
        return;
    }
    let total = cells.len() as f64;
    let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut wild = Vec::new();
    for &c in &cells {
        let r = p.cell[c];
        if r == x {
            wild.push(c);
        } else {
            groups.entry(geo.ancestor_at(r, reg.depth + 1)).or_default().push(c);
        }
    }
    let share = |id: u32| geo.regions[id as usize].size as f64 / reg.size as f64 * total;
    let mut fresh: Vec<u32> = reg.children.iter().copied().filter(|c| !groups.contains_key(c) && share(*c) >= MIN_CELLS).collect();
    if !fresh.is_empty() && !wild.is_empty() {
        // Seeds sorted by age along the region's long axis, so recent siblings cluster into ridges.
        fresh.sort_by(|a, b| geo.regions[*a as usize].mtime.total_cmp(&geo.regions[*b as usize].mtime));
        let m = wild.len() as f64;
        let (mx, my) = wild.iter().fold((0.0, 0.0), |(a, b), &c| (a + p.pts[c][0], b + p.pts[c][1]));
        let (mx, my) = (mx / m, my / m);
        let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
        for &c in &wild {
            let (dx, dy) = (p.pts[c][0] - mx, p.pts[c][1] - my);
            sxx += dx * dx;
            syy += dy * dy;
            sxy += dx * dy;
        }
        let mut rng = Rng::new(hash_str(&reg.path) ^ p.root as u64);
        let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy) + if rng.f64() < 0.5 { PI } else { 0.0 };
        let (ax, ay) = (angle.cos(), angle.sin());
        let mut order: Vec<(f64, f64, usize)> = wild
            .iter()
            .map(|&c| {
                let (dx, dy) = (p.pts[c][0] - mx, p.pts[c][1] - my);
                (dx * ax + dy * ay, (dy * ax - dx * ay).abs(), c)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let spread = order.iter().map(|o| o.1).fold(0.0, f64::max);
        let sum: f64 = fresh.iter().map(|&c| share(c)).sum();
        let window = (order.len() / 30).max(2);
        let (mut acc, mut used, mut seeds) = (0.0, HashSet::new(), Vec::new());
        for &id in &fresh {
            let b = share(id);
            let f = ((acc + b / 2.0) / sum).clamp(0.0, 1.0);
            acc += b;
            let mid = (f * (order.len() - 1) as f64) as usize;
            let target = rng.f64() * 0.5 * spread;
            let (lo, hi) = (mid.saturating_sub(window), (mid + window).min(order.len() - 1));
            let pick = (lo..=hi)
                .filter(|&i| !used.contains(&order[i].2))
                .min_by(|&i, &j| (order[i].1 - target).abs().total_cmp(&(order[j].1 - target).abs()))
                .map(|i| order[i].2)
                .or_else(|| order.iter().map(|o| o.2).find(|c| !used.contains(c)));
            if let Some(c) = pick {
                used.insert(c);
                seeds.push(Seed { region: id, starts: vec![c], budget: b, spine: None });
            }
        }
        let freq = 1.0 / (total.sqrt() * p.s * 0.6);
        let fine = 1.0 / (p.s * 5.0);
        let o = (hash_str(&reg.path) % 1000) as f64;
        // Region-scale noise shapes the territory; cell-scale noise breaks up the hex lattice on small regions.
        grow(p, &seeds, x, false, &|p, c| {
            let [px, py] = p.pts[c];
            0.3 + 1.4 * fbm01(perlin, px * freq + o, py * freq, 3) + 0.9 * fbm01(perlin, px * fine - o, py * fine, 2)
        });
        seeds.iter().for_each(|s| {
            p.placed.insert(s.region);
        });
        for &c in &wild {
            if p.cell[c] != x {
                groups.entry(p.cell[c]).or_default().push(c);
            }
        }
    }
    for (child, cs) in groups {
        layout(geo, p, child, cs, perlin);
    }
}

pub fn build_world(geo: &Geo) -> Patch {
    // Mesh a little past the map edge so the outermost cells cover it fully; rendering clips at the edge.
    let pad = 1.5 * (WORLD_W * WORLD_H / CELLS as f64).sqrt();
    let area = (WORLD_W + 2.0 * pad) * (WORLD_H + 2.0 * pad) / (WORLD_W * WORLD_H);
    let mut p = mesh(WORLD, "world", -pad, -pad, WORLD_W + 2.0 * pad, WORLD_H + 2.0 * pad, (CELLS as f64 * area) as usize, |_| true);
    let perlin = Perlin::new(hash_str(&geo.root) as u32);
    let n = p.pts.iter().filter(|q| (0.0..WORLD_W).contains(&q[0]) && (0.0..WORLD_H).contains(&q[1])).count() as f64;
    let cap = geo.capacity.max(1) as f64;
    let budget = |id: u32| geo.regions[id as usize].size as f64 / cap * n;
    let kids = geo.regions[WORLD as usize].children.clone();
    let of_kind = |k: RKind| kids.iter().copied().filter(move |&c| geo.regions[c as usize].kind == k).collect::<Vec<_>>();
    let mut rng = Rng::new(hash_str(&geo.root));
    let off = rng.f64() * 100.0;

    // System files are two Mercator ice bands along the north and south edges, with a noisy shoreline.
    let mut cap = 0.0;
    for id in of_kind(RKind::Polar) {
        let half = (budget(id) / 2.0).round() as usize;
        cap = half as f64 / n * WORLD_H;
        for (k, north) in [true, false].into_iter().enumerate() {
            let mut cells: Vec<(f64, usize)> = (0..p.len())
                .filter(|&c| p.cell[c] == NONE)
                .map(|c| {
                    let [x, y] = p.pts[c];
                    let pole = if north { y } else { WORLD_H - y };
                    (pole + 0.07 * (fbm01(&perlin, x * 6.0 + off, y * 6.0 + 40.0 * k as f64, 4) - 0.5), c)
                })
                .collect();
            cells.sort_by(|a, b| a.0.total_cmp(&b.0));
            cells.iter().take(half).for_each(|&(_, c)| p.cell[c] = id);
        }
    }

    let mut placed: Vec<([f64; 2], f64)> = Vec::new();
    let mut seeds = Vec::new();
    for id in of_kind(RKind::Continent) {
        let b = budget(id);
        let rad = (b / n * WORLD_W * WORLD_H / PI).sqrt();
        let mut best = None;
        let mut best_score = f64::MIN;
        for _ in 0..80 {
            let (mx, my) = ((0.1 + rad).min(WORLD_W / 2.0), (0.08 + cap + rad).min(0.5));
            let (x, y) = (mx + rng.f64() * (WORLD_W - 2.0 * mx), my + rng.f64() * (WORLD_H - 2.0 * my).max(0.0));
            let Some(c) = p.nearest(x, y).filter(|&c| p.cell[c] == NONE) else { continue };
            let gap = placed.iter().map(|(q, qr)| (q[0] - x).hypot(q[1] - y) - qr - rad).fold(1.0, f64::min);
            let centre = (x - WORLD_W / 2.0).hypot(y - 0.45);
            let score = gap - 0.5 * centre * rad;
            if score > best_score {
                best_score = score;
                best = Some(c);
            }
        }
        if let Some(c) = best {
            placed.push((p.pts[c], rad));
            seeds.push(Seed { region: id, starts: vec![c], budget: b, spine: Some(Spine::new(p.pts[c], rad, &mut rng)) });
        }
    }
    // Smooth zones grow round, Africa-like coasts; rough zones follow warped arcs (peninsulas) with twigs
    // branching only off those arcs (peninsulas of peninsulas), like Europe or Arabia.
    let cont_cost = |p: &Patch, c: usize| {
        let [x, y] = p.pts[c];
        let edge = x.min(WORLD_W - x).min(y).min(WORLD_H - y);
        let rough = roughness(&perlin, off, x, y);
        let wx = x + 0.09 * perlin.get([x * 2.3 + off, y * 2.3]);
        let wy = y + 0.09 * perlin.get([x * 2.3, y * 2.3 - off]);
        let arc = 1.0 - perlin.get([wx * 3.2 + off, wy * 3.2]).abs();
        let twig = (1.0 - perlin.get([wx * 9.0 - off, wy * 9.0 + 3.0]).abs()) * smoothstep(0.55, 0.85, arc);
        let ridge = arc.max(0.9 * twig);
        let jagged = 0.06 + 9.0 * (1.0 - ridge).powi(3);
        let round = 0.5 + 1.6 * fbm01(&perlin, x * 9.0 - off, y * 9.0 + 5.0, 3).powi(2);
        let lattice = 0.5 * fbm01(&perlin, x * 45.0 - off, y * 45.0, 2);
        (round + (jagged - round) * rough + lattice) * (1.0 + 400.0 * (0.09 - edge).max(0.0))
    };
    let got = grow(&mut p, &seeds, NONE, true, &cont_cost);
    // Nearly full disks can't fit separated continents: let the deficit grow into neighbours.
    let retry: Vec<Seed> = seeds
        .iter()
        .zip(&got)
        .filter(|(s, g)| (**g as f64) < s.budget - 1.0)
        .map(|(s, &g)| Seed { region: s.region, starts: frontier(&p, s.region, NONE), budget: s.budget - g as f64, spine: None })
        .collect();
    if !retry.is_empty() {
        grow(&mut p, &retry, NONE, false, &cont_cost);
    }

    fray(geo, &mut p, &perlin, off);
    let dland = bfs(&p, |c| p.cell[c] != NONE, |c| p.cell[c] == NONE);
    let mut isle_pts: Vec<[f64; 2]> = Vec::new();
    for id in of_kind(RKind::Island) {
        let b = budget(id).max(1.0);
        let rc = (b / PI).sqrt();
        let want = 5.0 + 1.5 * rc;
        let mut best = None;
        let mut best_score = f64::MIN;
        for _ in 0..120 {
            let c = rng.below(p.len());
            if p.cell[c] != NONE || dland[c] == u32::MAX {
                continue;
            }
            let [x, y] = p.pts[c];
            let spacing = isle_pts.iter().map(|q| (q[0] - x).hypot(q[1] - y) / p.s).fold(25.0, f64::min);
            let score = -(dland[c] as f64 - want).abs() + 0.4 * spacing;
            if score > best_score {
                best_score = score;
                best = Some(c);
            }
        }
        if let Some(c) = best {
            isle_pts.push(p.pts[c]);
            let spine = Spine::new(p.pts[c], (b / n * WORLD_W * WORLD_H / PI).sqrt(), &mut rng);
            grow(&mut p, &[Seed { region: id, starts: vec![c], budget: b, spine: Some(spine) }], NONE, true, &cont_cost);
        }
    }

    // Dust becomes coastline: tiny islets hugging and fraying the shores.
    if let Some(&id) = of_kind(RKind::Isles).first() {
        let mut left = budget(id).round() as i64;
        let coast: Vec<usize> = (0..p.len())
            .filter(|&c| {
                p.cell[c] == NONE
                    && p.nbrs[c].iter().any(|&q| {
                        let r = p.cell[q as usize];
                        r != NONE && geo.regions[r as usize].kind != RKind::Polar
                    })
            })
            .collect();
        let mut tries = 0;
        while left > 0 && !coast.is_empty() && tries < 50_000 {
            tries += 1;
            let mut c = coast[rng.below(coast.len())];
            for _ in 0..rng.below(5) {
                let opts: Vec<usize> = p.nbrs[c].iter().map(|&q| q as usize).filter(|&q| p.cell[q] == NONE && dland[q] >= dland[c]).collect();
                if opts.is_empty() {
                    break;
                }
                c = opts[rng.below(opts.len())];
            }
            if p.cell[c] != NONE {
                continue;
            }
            let size = 1 + rng.below((left as usize).min(6));
            let (mut q, mut k) = (VecDeque::from([c]), 0);
            while let Some(x) = q.pop_front() {
                if k >= size {
                    break;
                }
                if p.cell[x] != NONE {
                    continue;
                }
                p.cell[x] = id;
                k += 1;
                p.nbrs[x].iter().filter(|&&y| p.cell[y as usize] == NONE).for_each(|&y| q.push_back(y as usize));
            }
            left -= k as i64;
        }
    }

    if let Some(&id) = of_kind(RKind::Ocean).first() {
        p.cell.iter_mut().filter(|c| **c == NONE).for_each(|c| *c = id);
    }

    for id in kids {
        let cells: Vec<usize> = (0..p.len()).filter(|&c| p.cell[c] == id).collect();
        layout(geo, &mut p, id, cells, &perlin);
    }
    world_terrain(geo, &mut p, &perlin);
    finish(geo, &mut p, &perlin);
    p
}

/// 0 where coasts should be smooth and rounded, 1 where they should be jagged; varies over continental scales.
fn roughness(perlin: &Perlin, off: f64, x: f64, y: f64) -> f64 {
    smoothstep(0.4, 0.6, fbm01(perlin, x * 2.4 - off * 0.5, y * 2.4 + 17.0, 2))
}

/// Coastline greebling: carve fjords and grow capes where coast noise is strong, trading cells one for one
/// so every continent keeps its exact area. Never lets two landmasses touch.
fn fray(geo: &Geo, p: &mut Patch, perlin: &Perlin, off: f64) {
    let land = |r: u32| r != NONE && matches!(geo.regions[r as usize].kind, RKind::Continent | RKind::Island);
    for pass in 0..4 {
        let f = 11.0 + pass as f64 * 6.0;
        let n = |p: &Patch, c: usize| perlin.get([p.pts[c][0] * f + off * 3.0, p.pts[c][1] * f - pass as f64]);
        let mut balance: HashMap<u32, i64> = HashMap::new();
        let coast: Vec<usize> = (0..p.len()).filter(|&c| land(p.cell[c]) && p.nbrs[c].iter().any(|&q| p.cell[q as usize] == NONE)).collect();
        for c in coast {
            let rough = roughness(perlin, off, p.pts[c][0], p.pts[c][1]);
            if n(p, c) < -0.15 - 0.25 * (1.0 - rough) && p.nbrs[c].iter().filter(|&&q| p.cell[q as usize] == p.cell[c]).count() >= 2 {
                *balance.entry(p.cell[c]).or_default() += 1;
                p.cell[c] = NONE;
            }
        }
        let shore: Vec<usize> = (0..p.len()).filter(|&c| p.cell[c] == NONE && p.nbrs[c].iter().any(|&q| land(p.cell[q as usize]))).collect();
        let mut shore: Vec<(f64, usize)> = shore.into_iter().map(|c| (-n(p, c), c)).collect();
        shore.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, c) in shore {
            let owners: HashSet<u32> = p.nbrs[c].iter().map(|&q| p.cell[q as usize]).filter(|&r| r != NONE).collect();
            let Some(&r) = owners.iter().next() else { continue };
            if owners.len() != 1 || !land(r) || balance.get(&r).copied().unwrap_or(0) <= 0 {
                continue;
            }
            p.cell[c] = r;
            *balance.get_mut(&r).unwrap() -= 1;
        }
    }
}

fn world_terrain(geo: &Geo, p: &mut Patch, perlin: &Perlin) {
    let water: Vec<bool> = p.cell.iter().map(|&r| r == NONE || geo.regions[r as usize].kind == RKind::Ocean).collect();
    let land: Vec<bool> = water.iter().map(|w| !w).collect();
    let mut rec = recency(geo, p);
    diffuse(p, &mut rec, &land, 8);
    let dcoast = bfs(p, |c| land[c] && p.nbrs[c].iter().any(|&q| water[q as usize]), |c| land[c]);
    let dsea = bfs(p, |c| water[c] && p.nbrs[c].iter().any(|&q| land[q as usize]), |c| water[c]);
    p.ice = p.cell.iter().map(|&r| r != NONE && geo.top(r).kind == RKind::Polar).collect();
    let oct = p.octaves;
    p.elev = (0..p.len())
        .into_par_iter()
        .map(|c| {
            let [x, y] = p.pts[c];
            let e = if water[c] {
                let d = dsea[c].min(60) as f64;
                (-(0.03 + 0.97 * (d / 30.0).min(1.0)) + 0.03 * perlin.get([x * 20.0, y * 20.0])).min(-0.01)
            } else {
                let falloff = 0.2 + 0.8 * smoothstep(0.0, 8.0, dcoast[c] as f64);
                let r = rec[c] as f64;
                let base = if p.ice[c] {
                    0.14 + 0.3 * fbm01(perlin, x * 6.0, y * 6.0 + 9.0, 5)
                } else {
                    0.04 + 0.3 * r + detail(perlin, x, y, 0, oct, r)
                };
                (base * falloff).max(0.006)
            };
            e as f32
        })
        .collect();
    // Shoreline cells take their height from a smoothed land/sea field, so the drawn coast curves between cell
    // centres instead of stepping along the lattice.
    let mut landf: Vec<f32> = land.iter().map(|&l| l as u8 as f32).collect();
    diffuse(p, &mut landf, &p.inside.clone(), 2);
    for c in 0..p.len() {
        let d = if land[c] { dcoast[c] } else { dsea[c] };
        if d <= 1 {
            let t = if d == 0 { 0.0 } else { 0.5 };
            p.elev[c] = (landf[c] - 0.5) * 0.12 * (1.0 - t) + p.elev[c] * t;
        }
    }
    p.rec = rec;
}

/// Finer patch for region `r`, inheriting the parent's layout and adding the next levels of detail.
pub fn build_patch(geo: &Geo, parent: &Patch, r: u32) -> Patch {
    let cells: Vec<usize> = (0..parent.len()).filter(|&c| parent.inside[c] && geo.is_within(parent.cell[c], r)).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &c in &cells {
        let [x, y] = parent.pts[c];
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let pad = parent.s * 1.5;
    let (x0, y0, w, h) = (x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad);
    let fill = (cells.len() as f64 * parent.s * parent.s / (w * h)).clamp(0.02, 1.0);
    let n = ((CELLS as f64 / fill) as usize).min(MAX_POINTS);
    let owner = |pt: [f64; 2]| parent.nearest(pt[0], pt[1]).filter(|&q| parent.inside[q] && geo.is_within(parent.cell[q], r));
    let mut p = mesh(r, &format!("region{r}"), x0, y0, w, h, n, |pt| owner(pt).is_some());
    let owners: Vec<(usize, usize)> = p.pts.par_iter().map(|&pt| owner(pt).map_or((0, usize::MAX), |q| (0, q))).collect();
    inherit(&mut p, &[parent], &owners);
    let perlin = Perlin::new(hash_str(&geo.root) as u32);
    let all: Vec<usize> = (0..p.len()).filter(|&c| p.inside[c]).collect();
    layout(geo, &mut p, r, all, &perlin);
    fine_terrain(geo, &mut p, &[parent], &owners, &perlin);
    finish(geo, &mut p, &perlin);
    p
}

/// Square quadtree tile adding terrain detail beyond what the region hierarchy provides.
/// Each point inherits from the finest of `parents` (sorted fine to coarse) that has land there.
pub fn build_tile(geo: &Geo, parents: &[&Patch], level: u32, ix: i64, iy: i64) -> Patch {
    let size = 0.5f64.powi(level as i32);
    let owner = |pt: [f64; 2]| parents.iter().enumerate().find_map(|(i, par)| par.land_at(geo, pt).map(|q| (i, q)));
    let mut p = mesh(NONE, &format!("tile{level}/{ix}/{iy}"), ix as f64 * size, iy as f64 * size, size, size, CELLS, |pt| owner(pt).is_some());
    p.level = Some(level);
    let owners: Vec<(usize, usize)> = p.pts.par_iter().map(|&pt| owner(pt).unwrap_or((0, usize::MAX))).collect();
    inherit(&mut p, parents, &owners);
    let perlin = Perlin::new(hash_str(&geo.root) as u32);
    fine_terrain(geo, &mut p, parents, &owners, &perlin);
    p.shade = shade(&p);
    p
}

fn inherit(p: &mut Patch, parents: &[&Patch], owners: &[(usize, usize)]) {
    for c in 0..p.len() {
        if p.inside[c] {
            let (i, q) = owners[c];
            p.cell[c] = parents[i].cell[q];
            p.ice[c] = parents[i].ice[q];
        }
    }
}

fn fine_terrain(geo: &Geo, p: &mut Patch, parents: &[&Patch], owners: &[(usize, usize)], perlin: &Perlin) {
    let mut rec = recency(geo, p);
    let inside = p.inside.clone();
    diffuse(p, &mut rec, &inside, 8);
    let dedge = bfs(p, |c| p.edge[c], |c| inside[c]);
    let k1 = p.octaves;
    let mut used = vec![false; parents.len()];
    (0..p.len()).filter(|&c| inside[c]).for_each(|c| used[owners[c].0] = true);
    // Triangles around each parent vertex, flattened: vertex v owns tris[start[v]..start[v + 1]].
    let incidents: Vec<(Vec<u32>, Vec<u32>)> = parents
        .par_iter()
        .zip(&used)
        .map(|(par, &u)| {
            if !u {
                return (Vec::new(), Vec::new());
            }
            let mut start = vec![0u32; par.len() + 1];
            par.tris.iter().for_each(|&v| start[v as usize + 1] += 1);
            for v in 0..par.len() {
                start[v + 1] += start[v];
            }
            let mut fill = start.clone();
            let mut tris = vec![0u32; par.tris.len()];
            for (i, &v) in par.tris.iter().enumerate() {
                tris[fill[v as usize] as usize] = (i / 3) as u32;
                fill[v as usize] += 1;
            }
            (start, tris)
        })
        .collect();
    p.elev = (0..p.len())
        .into_par_iter()
        .map(|c| {
            if !inside[c] {
                return 0.0;
            }
            let [x, y] = p.pts[c];
            let (pi, q) = owners[c];
            let (parent, k0) = (parents[pi], parents[pi].octaves);
            let (mut e, mut rp) = (0.0, 0.0);
            for (k, w) in locate(parent, &incidents[pi], q, [x, y]) {
                e += w * parent.elev[k] as f64;
                rp += w * parent.rec[k] as f64;
            }
            if e <= 0.0 || p.ice[c] {
                return e as f32;
            }
            let r = rec[c] as f64;
            let taper = smoothstep(0.0, 12.0, dedge[c] as f64);
            (e + taper * 0.3 * (r - rp) + detail(perlin, x, y, k0, k1, r)).max(e * 0.5) as f32
        })
        .collect();
    p.rec = rec;
}

/// Barycentric weights of `pt` within the parent triangle containing it, matching the GPU's interpolation of the coarse mesh.
fn locate(parent: &Patch, (start, incident): &(Vec<u32>, Vec<u32>), q: usize, pt: [f64; 2]) -> Vec<(usize, f64)> {
    for v in std::iter::once(q).chain(parent.nbrs[q].iter().map(|&k| k as usize)) {
        for &t in &incident[start[v] as usize..start[v + 1] as usize] {
            let t = t as usize * 3;
            let [a, b, c] = [parent.tris[t], parent.tris[t + 1], parent.tris[t + 2]].map(|i| i as usize);
            let ([x0, y0], [x1, y1], [x2, y2]) = (parent.pts[a], parent.pts[b], parent.pts[c]);
            let d = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
            let wa = ((y1 - y2) * (pt[0] - x2) + (x2 - x1) * (pt[1] - y2)) / d;
            let wb = ((y2 - y0) * (pt[0] - x2) + (x0 - x2) * (pt[1] - y2)) / d;
            let wc = 1.0 - wa - wb;
            if wa >= -1e-9 && wb >= -1e-9 && wc >= -1e-9 {
                return vec![(a, wa), (b, wb), (c, wc)];
            }
        }
    }
    vec![(q, 1.0)]
}

fn finish(geo: &Geo, p: &mut Patch, perlin: &Perlin) {
    p.shade = shade(p);
    p.borders = borders(geo, p, perlin);
    if p.root == WORLD {
        p.rivers = rivers(p, perlin);
    }
    places(geo, p);
}

fn shade(p: &Patch) -> Vec<f32> {
    let l = [-1.0, -1.0, 1.4];
    let ll = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]) as f64;
    let ll = ll.sqrt();
    (0..p.len())
        .into_par_iter()
        .map(|c| {
            if !p.inside[c] {
                return 0.0;
            }
            let (mut a, mut b, mut d, mut bx, mut by) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for &q in &p.nbrs[c] {
                let q = q as usize;
                let (dx, dy) = (p.pts[q][0] - p.pts[c][0], p.pts[q][1] - p.pts[c][1]);
                let de = (p.elev[q] - p.elev[c]) as f64;
                a += dx * dx;
                b += dx * dy;
                d += dy * dy;
                bx += dx * de;
                by += dy * de;
            }
            let det = a * d - b * b;
            let (gx, gy) = if det.abs() < 1e-30 { (0.0, 0.0) } else { ((d * bx - b * by) / det, (a * by - b * bx) / det) };
            let k = 0.06;
            let (nx, ny, nz) = (-gx * k, -gy * k, 1.0);
            let nl = (nx * nx + ny * ny + nz * nz).sqrt();
            ((nx * l[0] + ny * l[1] + nz * l[2]) / (nl * ll)).max(0.0) as f32
        })
        .collect()
}

fn circumcenter(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> [f64; 2] {
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

fn ancestry(geo: &Geo, mut r: u32) -> Vec<u32> {
    let mut v = Vec::new();
    while r != NONE {
        v.push(r);
        r = geo.regions[r as usize].parent;
    }
    v.reverse();
    v
}

/// Midpoint-displaced polyline so borders look surveyed rather than polygonal.
fn jagged(out: &mut Vec<f32>, a: [f64; 2], b: [f64; 2], level: f32, perlin: &Perlin, depth: u32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if depth == 0 || len < 1e-12 {
        out.extend([a[0] as f32, a[1] as f32, b[0] as f32, b[1] as f32, level]);
        return;
    }
    let k = 0.37 / len;
    let off = perlin.get([(a[0] + b[0]) * k + 3.3, (a[1] + b[1]) * k - 1.7]) * 0.35;
    let m = [(a[0] + b[0]) / 2.0 - dy * off, (a[1] + b[1]) / 2.0 + dx * off];
    jagged(out, a, m, level, perlin, depth - 1);
    jagged(out, m, b, level, perlin, depth - 1);
}

fn borders(geo: &Geo, p: &Patch, perlin: &Perlin) -> Vec<f32> {
    let (t, he) = (&p.triangles, &p.halfedges);
    let cc = |tri: usize| circumcenter(p.pts[t[3 * tri]], p.pts[t[3 * tri + 1]], p.pts[t[3 * tri + 2]]);
    let mut chains: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut out = Vec::new();
    for e in 0..he.len() {
        let o = he[e];
        if o == EMPTY || o < e || !p.good[e / 3] || !p.good[o / 3] {
            continue;
        }
        let (a, b) = (t[e], t[if e % 3 == 2 { e - 2 } else { e + 1 }]);
        if !p.inside[a] || !p.inside[b] {
            continue;
        }
        let (ra, rb) = (p.cell[a], p.cell[b]);
        if ra == rb || [ra, rb].iter().any(|&r| matches!(geo.regions[r as usize].kind, RKind::Ocean | RKind::Isles)) {
            continue;
        }
        let ca = chains.entry(ra).or_insert_with(|| ancestry(geo, ra)).clone();
        let cb = chains.entry(rb).or_insert_with(|| ancestry(geo, rb));
        let d = ca.iter().zip(cb.iter()).take_while(|(x, y)| x == y).count();
        let fresh = |x: Option<&u32>| x.is_some_and(|x| p.placed.contains(x));
        if p.root != WORLD && !fresh(ca.get(d)) && !fresh(cb.get(d)) {
            continue;
        }
        jagged(&mut out, cc(e / 3), cc(o / 3), d as f32, perlin, 2);
    }
    out
}

fn rivers(p: &Patch, perlin: &Perlin) -> Vec<f32> {
    let land = |c: usize| p.inside[c] && p.elev[c] > 0.0 && !p.ice[c];
    let (mut down, mut done, mut order) = (vec![usize::MAX; p.len()], vec![false; p.len()], Vec::new());
    let mut heap = BinaryHeap::new();
    for c in 0..p.len() {
        if !land(c) {
            continue;
        }
        if let Some(&o) = p.nbrs[c].iter().filter(|&&q| p.elev[q as usize] <= 0.0).min_by(|a, b| p.elev[**a as usize].total_cmp(&p.elev[**b as usize])) {
            down[c] = o as usize;
            done[c] = true;
            heap.push(Item(p.elev[c] as f64, c as u32, 0));
        }
    }
    // Priority flood: drains every basin to the sea without carving lakes.
    while let Some(Item(h, c, _)) = heap.pop() {
        let c = c as usize;
        order.push(c);
        for &q in &p.nbrs[c] {
            let q = q as usize;
            if !done[q] && land(q) {
                done[q] = true;
                down[q] = c;
                heap.push(Item((p.elev[q] as f64).max(h + 1e-7), q as u32, 0));
            }
        }
    }
    let mut flux = vec![0.0f64; p.len()];
    for &c in order.iter().rev() {
        flux[c] += 1.0 + 2.0 * p.elev[c].max(0.0) as f64;
        let d = down[c];
        if d != usize::MAX && land(d) {
            flux[d] += flux[c];
        }
    }
    let threshold = (p.len() as f64 * 0.0012).max(40.0);
    let mut out = Vec::new();
    for c in 0..p.len() {
        let d = down[c];
        if !land(c) || flux[c] < threshold || d == usize::MAX {
            continue;
        }
        let to = if land(d) { p.pts[d] } else { [(p.pts[c][0] + p.pts[d][0]) / 2.0, (p.pts[c][1] + p.pts[d][1]) / 2.0] };
        jagged(&mut out, p.pts[c], to, (flux[c] / threshold).sqrt() as f32, perlin, 3);
    }
    out
}

/// Label anchors (pole of inaccessibility per region and depth) and city positions.
fn places(geo: &Geo, p: &mut Patch) {
    let world = p.root == WORLD;
    let wanted = |r: u32| world || p.placed.contains(&r);
    let maxd = p.cell.iter().filter(|&&r| r != NONE).map(|&r| geo.regions[r as usize].depth).max().unwrap_or(0);
    let shallow = NONE - 1;
    let mut acc: HashMap<u32, Label> = HashMap::new();
    let mut best: HashMap<u32, u32> = HashMap::new();
    let layers: Vec<(Vec<u32>, Vec<u32>)> = (1..=maxd)
        .into_par_iter()
        .map(|d| {
            let key: Vec<u32> = p
                .cell
                .iter()
                .map(|&r| match r {
                    NONE => NONE,
                    r if geo.regions[r as usize].depth >= d => geo.ancestor_at(r, d),
                    _ => shallow,
                })
                .collect();
            let valid = |c: usize| key[c] < shallow;
            let dist = bfs(p, |c| valid(c) && (p.edge[c] || p.nbrs[c].iter().any(|&q| key[q as usize] != key[c])), valid);
            (key, dist)
        })
        .collect();
    for (key, dist) in layers {
        let valid = |c: usize| key[c] < shallow;
        for c in 0..p.len() {
            let k = key[c];
            if !valid(c) || !wanted(k) || geo.regions[k as usize].kind == RKind::Isles {
                continue;
            }
            let [x, y] = p.pts[c];
            let l = acc.entry(k).or_insert(Label { id: k, x, y, r: 0.0, bbox: [x, y, x, y], cells: 0 });
            l.cells += 1;
            l.bbox = [l.bbox[0].min(x), l.bbox[1].min(y), l.bbox[2].max(x), l.bbox[3].max(y)];
            let b = best.entry(k).or_insert(0);
            if dist[c] >= *b {
                *b = dist[c];
                (l.x, l.y, l.r) = (x, y, (dist[c] as f64 + 0.5) * p.s);
            }
        }
    }
    let mut labels: Vec<Label> = acc.into_values().collect();
    labels.sort_by_key(|l| l.id);

    let kids = with_kids(geo, p);
    let dist = bfs(p, |c| p.cell[c] != NONE && (p.edge[c] || p.nbrs[c].iter().any(|&q| p.cell[q as usize] != p.cell[c])), |c| p.cell[c] != NONE);
    let mut cities: HashMap<u32, (u32, usize)> = HashMap::new();
    for c in 0..p.len() {
        let r = p.cell[c];
        if r == NONE || geo.regions[r as usize].city.is_none() || !(wanted(r) || r == p.root) {
            continue;
        }
        let e = cities.entry(r).or_insert((0, c));
        if dist[c] > e.0 {
            *e = (dist[c], c);
        }
    }
    let mut cities: Vec<City> = cities
        .into_iter()
        .map(|(id, (_, c))| {
            let reg = &geo.regions[id as usize];
            City { id, x: p.pts[c][0], y: p.pts[c][1], pop: if kids.contains(&id) { reg.dust.count } else { reg.count } }
        })
        .filter(|c| c.pop > 0)
        .collect();
    cities.sort_by_key(|c| c.id);
    p.labels = labels;
    p.cities = cities;
}

fn f32s(out: &mut Vec<u8>, v: impl IntoIterator<Item = f32>) {
    v.into_iter().for_each(|x| out.extend(x.to_le_bytes()));
}

fn u32s(out: &mut Vec<u8>, v: impl IntoIterator<Item = u32>) {
    v.into_iter().for_each(|x| out.extend(x.to_le_bytes()));
}

/// Binary payload: "ATL1", u32 JSON length, JSON header (padded to 4 bytes), then typed arrays.
pub fn encode(geo: &Geo, p: &Patch) -> Vec<u8> {
    let world = (p.root == WORLD).then(|| json!({ "capacity": geo.capacity, "disk": geo.disk, "scanned": geo.scanned, "free": geo.free, "now": geo.now, "root": geo.root, "regions": geo.regions }));
    let meta = json!({
        "root": p.root,
        "depth": p.level.map_or_else(|| geo.regions[p.root as usize].depth as u32, |l| 100 + l),
        "level": p.level,
        "ox": p.ox, "oy": p.oy, "s": p.s, "cols": p.cols, "rows": p.rows,
        "ntri": p.tris.len() / 3,
        "nborder": p.borders.len() / 5,
        "nriver": p.rivers.len() / 5,
        "labels": p.labels.iter().map(|l| json!([l.id, l.x, l.y, l.r, l.bbox[0], l.bbox[1], l.bbox[2], l.bbox[3], l.cells])).collect::<Vec<_>>(),
        "cities": p.cities.iter().map(|c| json!([c.id, c.x, c.y, c.pop])).collect::<Vec<_>>(),
        "world": world.unwrap_or(Value::Null),
    });
    let js = serde_json::to_vec(&meta).unwrap();
    let mut out = Vec::with_capacity(js.len() + p.len() * 24 + p.tris.len() * 4);
    out.extend(b"ATL1");
    out.extend((js.len() as u32).to_le_bytes());
    out.extend(&js);
    while out.len() % 4 != 0 {
        out.push(b' ');
    }
    f32s(&mut out, p.pts.iter().flat_map(|q| [q[0] as f32, q[1] as f32]));
    f32s(&mut out, p.elev.iter().copied());
    f32s(&mut out, p.shade.iter().copied());
    f32s(&mut out, p.ice.iter().map(|&i| i as u8 as f32));
    u32s(&mut out, p.cell.iter().copied());
    u32s(&mut out, p.tris.iter().copied());
    f32s(&mut out, p.borders.iter().copied());
    f32s(&mut out, p.rivers.iter().copied());
    out
}

pub fn stats(p: &Patch) -> String {
    format!(
        "{} points, {} triangles, {} border segs, {} river segs, {} labels, {} cities, {} placed",
        p.inside.iter().filter(|&&i| i).count(),
        p.tris.len() / 3,
        p.borders.len() / 5,
        p.rivers.len() / 5,
        p.labels.len(),
        p.cities.len(),
        p.placed.len()
    )
}
