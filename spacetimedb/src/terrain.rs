use spacetimedb::{table, reducer, ReducerContext};
use crate::zone as _;

pub const CHUNK_SIZE: u32 = 32;

#[table(accessor = terrain_chunk, public)]
pub struct TerrainChunk {
    #[primary_key]
    #[auto_inc]
    pub id:          u64,
    #[index(btree)]
    pub zone_id:     u64,
    pub chunk_x:     u32,
    pub chunk_z:     u32,
    pub height_data: Vec<u8>,
    pub splat_data:  Vec<u8>,
}

#[reducer]
pub fn update_terrain_chunk(
    ctx: &ReducerContext,
    zone_id: u64,
    chunk_x: u32,
    chunk_z: u32,
    height_data: Vec<u8>,
    splat_data: Vec<u8>,
) -> Result<(), String> {
    if height_data.len() != 4096 {
        return Err(format!("height_data must be 4096 bytes, got {}", height_data.len()));
    }
    if splat_data.len() != 4096 {
        return Err(format!("splat_data must be 4096 bytes, got {}", splat_data.len()));
    }

    for chunk in height_data.chunks_exact(4) {
        let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !val.is_finite() {
            return Err(format!("height_data contains non-finite float value: {}", val));
        }
    }

    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;

    let max_cx = zone.terrain_width.div_ceil(CHUNK_SIZE);
    let max_cz = zone.terrain_height.div_ceil(CHUNK_SIZE);
    if chunk_x >= max_cx || chunk_z >= max_cz {
        return Err(format!("Chunk ({},{}) out of bounds ({},{})", chunk_x, chunk_z, max_cx, max_cz));
    }

    let existing = ctx.db.terrain_chunk().zone_id().filter(&zone_id)
        .find(|c| c.chunk_x == chunk_x && c.chunk_z == chunk_z)
        .ok_or_else(|| format!("Chunk ({},{}) not found in zone {}", chunk_x, chunk_z, zone_id))?;

    ctx.db.terrain_chunk().id().update(TerrainChunk {
        height_data,
        splat_data,
        ..existing
    });

    Ok(())
}
