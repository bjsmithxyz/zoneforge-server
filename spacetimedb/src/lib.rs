use spacetimedb::{table, reducer, ReducerContext, Identity, Table};

// Define a simple Player table.
// Note: #[table(...)] is the table attribute — do NOT also add #[derive(SpacetimeType)].
// SpacetimeType is only for custom embedded types used as fields inside table rows.
#[table(accessor = player, public)]
pub struct Player {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    // #[unique] creates an index so reducers can look up players by identity.
    #[unique]
    pub identity: Identity,
    pub name: String,
    pub zone_id: u64,
    pub position_x: f32,
    pub position_y: f32,
    pub health: i32,
    pub max_health: i32,
}

// Define a Zone table
#[table(accessor = zone, public)]
pub struct Zone {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub terrain_width:  u32,
    pub terrain_height: u32,
    pub water_level:    f32,
}

// Define a TerrainChunk table — stores height and splat data for 32×32 terrain sections
#[table(accessor = terrain_chunk, public)]
pub struct TerrainChunk {
    #[primary_key]
    #[auto_inc]
    pub id:          u64,
    #[index(btree)]
    pub zone_id:     u64,
    pub chunk_x:     u32,
    pub chunk_z:     u32,
    pub height_data: Vec<u8>,  // 32×32 f32 LE = 4096 bytes
    pub splat_data:  Vec<u8>,  // 32×32 × 4 u8 = 4096 bytes
}

// Reducer to create a new player
#[reducer]
pub fn create_player(ctx: &ReducerContext, name: String) {
    // Idempotent: skip if this identity already has a player row
    if ctx.db.player().identity().find(ctx.sender()).is_some() {
        log::info!("create_player: identity {} already exists, skipping", ctx.sender());
        return;
    }
    let player = Player {
        id: 0,
        identity: ctx.sender(),
        name,
        zone_id: 1,
        position_x: 0.0,
        position_y: 0.0,
        health: 100,
        max_health: 100,
    };
    ctx.db.player().insert(player);
    log::info!("Player created: {}", ctx.sender());
}

// Reducer to move a player
#[reducer]
pub fn move_player(ctx: &ReducerContext, new_x: f32, new_y: f32) {
    let player_identity = ctx.sender();

    // .identity() works here because the field is marked #[unique] above.
    if let Some(player) = ctx.db.player().identity().find(player_identity) {
        ctx.db.player().id().update(Player {
            position_x: new_x,
            position_y: new_y,
            ..player
        });
        log::info!("Player moved to ({}, {})", new_x, new_y);
    }
}

const CHUNK_SIZE: u32 = 32;

// Reducer to create a zone and initialise flat terrain chunks
#[reducer]
pub fn create_zone(
    ctx: &ReducerContext,
    name: String,
    terrain_width: u32,
    terrain_height: u32,
    water_level: f32,
) {
    let zone = Zone {
        id: 0,
        name: name.clone(),
        terrain_width,
        terrain_height,
        water_level,
    };
    let zone_row = ctx.db.zone().insert(zone);
    let zone_id = zone_row.id;

    // Initialise flat terrain chunks (height = water_level + 0.5, full Grass).
    let chunks_x = terrain_width.div_ceil(CHUNK_SIZE);
    let chunks_z = terrain_height.div_ceil(CHUNK_SIZE);

    let default_height = water_level + 0.5_f32;
    let height_bytes = default_height.to_le_bytes();

    // 32×32 floats — repeat the 4-byte LE representation 1024 times.
    let height_data: Vec<u8> = height_bytes.iter()
        .cycle()
        .take(4096)
        .cloned()
        .collect();

    // 32×32 × 4 channels — R=255 (Grass), G=0, B=0, A=0.
    let splat_data: Vec<u8> = (0..1024)
        .flat_map(|_| [255u8, 0, 0, 0])
        .collect();

    for cx in 0..chunks_x {
        for cz in 0..chunks_z {
            ctx.db.terrain_chunk().insert(TerrainChunk {
                id: 0,
                zone_id,
                chunk_x: cx,
                chunk_z: cz,
                height_data: height_data.clone(),
                splat_data: splat_data.clone(),
            });
        }
    }

    log::info!("Zone '{}' created with {}×{} chunks", name, chunks_x, chunks_z);
}

// Reducer to update terrain chunk height and splat data
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

    // Verify zone + chunk bounds.
    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;

    let max_cx = zone.terrain_width.div_ceil(CHUNK_SIZE);
    let max_cz = zone.terrain_height.div_ceil(CHUNK_SIZE);
    if chunk_x >= max_cx || chunk_z >= max_cz {
        return Err(format!("Chunk ({},{}) out of bounds ({},{})", chunk_x, chunk_z, max_cx, max_cz));
    }

    // Find and update the existing chunk.
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

// Define an EntityInstance table — tracks placed objects within a zone
#[table(accessor = entity_instance, public)]
pub struct EntityInstance {
    #[primary_key]
    #[auto_inc]
    pub id:           u64,
    pub zone_id:      u64,
    pub prefab_name:  String,
    pub position_x:   f32,
    pub position_y:   f32,
    pub elevation:    f32,   // world-space Y (vertical)
    pub entity_type:  String,
}

// Reducer to spawn an entity in a zone
#[reducer]
pub fn spawn_entity(
    ctx: &ReducerContext,
    zone_id: u64,
    prefab_name: String,
    x: f32,
    y: f32,
    elevation: f32,
    entity_type: String,
) -> Result<(), String> {
    if prefab_name.is_empty() {
        return Err("prefab_name cannot be empty".to_string());
    }
    ctx.db.entity_instance().insert(EntityInstance {
        id: 0,
        zone_id,
        prefab_name: prefab_name.clone(),
        position_x: x,
        position_y: y,
        elevation,
        entity_type,
    });
    log::info!("Entity '{}' spawned in zone {} at ({}, {}, {})", prefab_name, zone_id, x, y, elevation);
    Ok(())
}
