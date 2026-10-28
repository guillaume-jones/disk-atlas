//! The binary payload the frontend uploads straight to the GPU: "ATL1", a u32 JSON header length, the JSON header
//! padded to 4 bytes, then little-endian arrays: points (x, y f32), elevation, shade, ice (f32), owner (u32),
//! triangles (3 × u32), borders and rivers (5 × f32 per segment).
use super::places::{City, Label};
use super::{Overlay, Patch, Scope};
use crate::geo::{Geo, NO_REGION, Region, RegionId};
use serde::Serialize;

const MAGIC: &[u8; 4] = b"ATL1";
/// Tiles sort above every region patch when the frontend layers them by depth.
const TILE_DEPTH_BASE: u32 = 100;

#[derive(Serialize)]
struct Header<'a> {
    root: u32,
    depth: u32,
    level: Option<u32>,
    ox: f64,
    oy: f64,
    s: f64,
    cols: usize,
    rows: usize,
    ntri: usize,
    nborder: usize,
    nriver: usize,
    labels: &'a [Label],
    cities: &'a [City],
    world: Option<WorldHeader<'a>>,
}

#[derive(Serialize)]
struct WorldHeader<'a> {
    capacity: u64,
    disk: u64,
    scanned: u64,
    free: u64,
    now: f64,
    root: &'a str,
    regions: &'a [Region],
}

pub fn encode(geo: &Geo, patch: &Patch, overlay: &Overlay) -> serde_json::Result<Vec<u8>> {
    let mesh = &patch.mesh;
    let (root, depth, level) = match patch.scope {
        Scope::Region(id) => (u32::from(id), u32::from(geo.region(id).depth), None),
        Scope::Tile { level } => (NO_REGION, TILE_DEPTH_BASE + level, Some(level)),
    };
    let world = (patch.scope == Scope::Region(RegionId::WORLD)).then(|| WorldHeader {
        capacity: geo.capacity,
        disk: geo.disk,
        scanned: geo.scanned,
        free: geo.free,
        now: geo.now,
        root: &geo.root,
        regions: geo.regions(),
    });
    let header = Header {
        root,
        depth,
        level,
        ox: mesh.origin[0],
        oy: mesh.origin[1],
        s: mesh.spacing,
        cols: mesh.cols,
        rows: mesh.rows,
        ntri: mesh.tris.len() / 3,
        nborder: overlay.borders.len(),
        nriver: overlay.rivers.len(),
        labels: &overlay.labels,
        cities: &overlay.cities,
        world,
    };
    // Through a Value so object keys come out sorted, keeping payloads byte-stable however structs are declared.
    let json = serde_json::to_vec(&serde_json::to_value(&header)?)?;
    let arrays_len = mesh.len() * 24 + mesh.tris.len() * 4 + (overlay.borders.len() + overlay.rivers.len()) * 20;
    let mut out = Vec::with_capacity(json.len() + 12 + arrays_len);
    out.extend(MAGIC);
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(&json);
    while out.len() % 4 != 0 {
        out.push(b' ');
    }
    put_f32s(&mut out, mesh.pts.iter().flat_map(|p| [p[0] as f32, p[1] as f32]));
    put_f32s(&mut out, patch.terrain.elevation.iter().copied());
    put_f32s(&mut out, overlay.shade.iter().copied());
    put_f32s(&mut out, patch.ice.iter().map(|&ice| f32::from(u8::from(ice))));
    put_u32s(&mut out, patch.owners.iter().map(|o| o.map_or(NO_REGION, u32::from)));
    put_u32s(&mut out, mesh.tris.iter().copied());
    for segment in overlay.borders.iter().chain(&overlay.rivers) {
        put_f32s(
            &mut out,
            [segment.from[0], segment.from[1], segment.to[0], segment.to[1], segment.weight],
        );
    }
    Ok(out)
}

fn put_f32s(out: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    values.into_iter().for_each(|v| out.extend(v.to_le_bytes()));
}

fn put_u32s(out: &mut Vec<u8>, values: impl IntoIterator<Item = u32>) {
    values.into_iter().for_each(|v| out.extend(v.to_le_bytes()));
}
