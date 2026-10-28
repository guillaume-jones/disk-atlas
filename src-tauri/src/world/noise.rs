//! Noise fields shared by coastlines and terrain.
use noise::{NoiseFn, Perlin};

/// Frequency of the coarsest terrain octave, in cycles per map height.
const BASE_FREQ: f64 = 3.0;
/// The finest octave a mesh can show spans this many cells.
const CELLS_PER_OCTAVE_WAVE: f64 = 6.0;

pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Number of terrain octaves a mesh with this lattice spacing can resolve.
pub fn octaves(spacing: f64) -> u32 {
    ((1.0 / BASE_FREQ) / (CELLS_PER_OCTAVE_WAVE * spacing)).log2().ceil().max(1.0) as u32
}

/// Fractal Brownian motion remapped to [0, 1].
pub fn fbm01(perlin: &Perlin, x: f64, y: f64, octaves: u32) -> f64 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        sum += amplitude * perlin.get([x * frequency, y * frequency]);
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    (0.5 + 0.5 * sum / norm).clamp(0.0, 1.0)
}

/// Ridged multifractal octaves `from..to`. Mountains scale with recency `r`, so stale data stays flat.
pub fn mountains(perlin: &Perlin, x: f64, y: f64, from: u32, to: u32, r: f64) -> f64 {
    let wx = x + 0.06 * perlin.get([x * 2.1 + 40.0, y * 2.1]);
    let wy = y + 0.06 * perlin.get([x * 2.1, y * 2.1 - 40.0]);
    let ridge_height = 0.08 + 0.92 * r;
    let mut sum = 0.0;
    for i in from..to {
        let frequency = BASE_FREQ * 2f64.powi(i as i32);
        let amplitude = 0.5f64.powi(i as i32);
        let ridge = (1.0
            - perlin
                .get([wx * frequency + 11.3 * f64::from(i), wy * frequency - 7.1 * f64::from(i)])
                .abs())
        .powi(2);
        let hill = perlin.get([x * frequency * 1.7 + 101.0, y * frequency * 1.7 - 33.0]);
        sum += amplitude * (ridge_height * 0.5 * ridge + 0.1 * hill);
    }
    sum
}

/// 0 where coasts should be smooth and rounded, 1 where they should be jagged; varies over continental scales.
pub fn roughness(perlin: &Perlin, offset: f64, x: f64, y: f64) -> f64 {
    smoothstep(0.4, 0.6, fbm01(perlin, x * 2.4 - offset * 0.5, y * 2.4 + 17.0, 2))
}
