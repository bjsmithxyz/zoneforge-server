use spacetimedb::{table, reducer, ReducerContext, Identity, Table, ScheduleAt};

mod atmosphere;
mod terrain;
mod portal;
mod inventory;
mod loot;
mod combat;
mod enemy;
pub use atmosphere::{WeatherKind, WeatherState, WorldClock, WorldClockTick};
pub use terrain::{TerrainChunk, CHUNK_SIZE};
pub use portal::Portal;
pub use inventory::{ItemDefinition, Inventory, Equipment, ItemType, Rarity};
pub use loot::{LootTable, ItemDrop};
pub use combat::{Ability, PlayerCooldown, StatusEffect, CombatLog,
                 StatusEffectTick, ManaRegenTick, CombatLogPruneTick,
                 AbilityType, StatusEffectType};
pub use enemy::{EnemyDefinition, SpawnPoint, Enemy, EnemyRespawnTick, AiTick,
                AiState, EnemyType};
// Bring accessor traits into scope so ctx.db.<table>() works for moved tables.
// Imported as `_` to avoid colliding with module names of the same identifier.
use atmosphere::{weather_state as _, world_clock as _, world_clock_tick as _};
use terrain::terrain_chunk as _;
use portal::portal as _;
use loot::item_drop as _;
use combat::{ability as _, status_effect_tick as _, mana_regen_tick as _, combat_log_prune_tick as _};
use enemy::{spawn_point as _, enemy as _, enemy_respawn_tick as _, ai_tick as _};

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
    #[index(btree)]
    pub zone_id: u64,
    pub position_x: f32,
    pub position_y: f32,
    pub health: i32,
    pub max_health: i32,
    pub mana: i32,
    pub max_mana: i32,
    pub is_dead: bool,
}

// Admin table — one row per admin identity.
// Admin set is compile-time; seeded in init(). Changes require --delete-data republish.
#[table(accessor = admin, public)]
pub struct Admin {
    #[primary_key]
    pub identity: Identity,
}

// Run `spacetime login show` to get your 64-char identity hex.
// Add each admin identity (with or without "0x" prefix) before publishing.
const ADMIN_IDENTITIES: &[&str] = &[
    "0xc2007b97a0605a88c5ce60d229b1067a1bfeb27a37cf6371b5830c6b404932da",
    "0xc200496650f3c734cb567002e110e78f6876c4b17ca9512aed8927dd412a104b",
    "0xc200aa92d6daccb0a2bd52f1da120326f9ce92bab87abfe95e99e58e767de4d8",
];
const _: () = assert!(!ADMIN_IDENTITIES.is_empty(), "ADMIN_IDENTITIES must contain at least one entry");

fn identity_from_hex(hex: &str) -> Identity {
    let hex = hex.trim_start_matches("0x");
    assert!(hex.len() == 64, "Admin identity hex must be 64 characters (32 bytes)");
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .expect("ADMIN_IDENTITIES contains non-hex characters");
    }
    // `spacetime login show` outputs identity in big-endian (MSB first);
    // Identity::from_byte_array expects the bytes in SpacetimeDB's internal order (reversed).
    bytes.reverse();
    Identity::from_byte_array(bytes)
}

/// Returns true if ctx.sender() is in the Admin table.
fn is_admin(ctx: &ReducerContext) -> bool {
    ctx.db.admin().identity().find(ctx.sender()).is_some()
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
    pub mood_preset_id: u32,
}

/// Ensures all self-scheduling ticks are running after a hot-publish (init only runs on fresh databases).
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
    // Auto-promote compile-time admin identities (survives --delete-data)
    if ctx.db.admin().identity().find(ctx.sender()).is_none() {
        for &hex in ADMIN_IDENTITIES {
            if identity_from_hex(hex) == ctx.sender() {
                ctx.db.admin().insert(Admin { identity: ctx.sender() });
                log::info!("client_connected: auto-promoted admin {:?}", ctx.sender());
                break;
            }
        }
    }

    if ctx.db.status_effect_tick().iter().next().is_none() {
        ctx.db.status_effect_tick().insert(StatusEffectTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(1)
            ),
        });
        log::info!("client_connected: bootstrapped status effect tick");
    }
    if ctx.db.mana_regen_tick().iter().next().is_none() {
        ctx.db.mana_regen_tick().insert(ManaRegenTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(2)
            ),
        });
        log::info!("client_connected: bootstrapped mana regen tick");
    }
    if ctx.db.ai_tick().iter().next().is_none() {
        ctx.db.ai_tick().insert(AiTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_millis(500)
            ),
        });
        log::info!("client_connected: bootstrapped AI tick");
    }
    if ctx.db.combat_log_prune_tick().iter().next().is_none() {
        ctx.db.combat_log_prune_tick().insert(CombatLogPruneTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(60)
            ),
        });
        log::info!("client_connected: bootstrapped combat_log prune tick");
    }
}

pub(crate) fn dist_sq(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

pub(crate) fn step_toward(from_x: f32, from_y: f32, to_x: f32, to_y: f32, step: f32) -> (f32, f32) {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist <= step {
        (to_x, to_y)
    } else {
        (from_x + dx / dist * step, from_y + dy / dist * step)
    }
}

#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    // Seed compile-time admin identities (only on fresh databases)
    for &hex in ADMIN_IDENTITIES {
        ctx.db.admin().insert(Admin { identity: identity_from_hex(hex) });
    }
    log::info!("init: seeded {} admin identity(ies)", ADMIN_IDENTITIES.len());

    // Seed starter abilities if table is empty
    if ctx.db.ability().iter().next().is_none() {
        ctx.db.ability().insert(Ability {
            id: 0,
            name: "Auto-Attack".to_string(),
            damage: 20,
            cooldown_ms: 500,
            mana_cost: 0,
            range: 2.5,
            ability_type: AbilityType::MeleeAttack,
        });
        ctx.db.ability().insert(Ability {
            id: 0,
            name: "Fireball".to_string(),
            damage: 50,
            cooldown_ms: 3000,
            mana_cost: 20,
            range: 15.0,
            ability_type: AbilityType::Projectile,
        });
        ctx.db.ability().insert(Ability {
            id: 0,
            name: "Heal".to_string(),
            damage: -50,
            cooldown_ms: 10000,
            mana_cost: 30,
            range: 0.0,
            ability_type: AbilityType::SelfCast,
        });
        log::info!("init: seeded 3 abilities");
    }

    // Start the recurring status effect tick if not already scheduled
    if ctx.db.status_effect_tick().iter().next().is_none() {
        ctx.db.status_effect_tick().insert(StatusEffectTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(1)
            ),
        });
        log::info!("init: scheduled status effect tick");
    }

    // Start the mana regen tick
    if ctx.db.mana_regen_tick().iter().next().is_none() {
        ctx.db.mana_regen_tick().insert(ManaRegenTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(2)
            ),
        });
        log::info!("init: scheduled mana regen tick");
    }

    // Start the enemy AI tick
    if ctx.db.ai_tick().iter().next().is_none() {
        ctx.db.ai_tick().insert(AiTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_millis(500)
            ),
        });
        log::info!("init: scheduled AI tick");
    }

    // Start the combat_log prune tick (caps row count to keep client subs bounded)
    if ctx.db.combat_log_prune_tick().iter().next().is_none() {
        ctx.db.combat_log_prune_tick().insert(CombatLogPruneTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(60)
            ),
        });
        log::info!("init: scheduled combat_log prune tick");
    }

    // WorldClock bootstrap
    if ctx.db.world_clock().id().find(0u8).is_none() {
        ctx.db.world_clock().insert(WorldClock {
            id: 0,
            minutes_of_day: 480, // 08:00
            last_tick: ctx.timestamp,
        });
        log::info!("init: bootstrapped WorldClock");
    }
    // Start the world-time tick
    if ctx.db.world_clock_tick().iter().next().is_none() {
        ctx.db.world_clock_tick().insert(WorldClockTick {
            scheduled_id: 0,
            scheduled_at: ScheduleAt::Time(
                ctx.timestamp + std::time::Duration::from_secs(1)
            ),
        });
        log::info!("init: scheduled world time tick");
    }
}

// Reducer to create a new player
#[reducer]
pub fn create_player(ctx: &ReducerContext, name: String) {
    // Validate name: non-empty, max 64 bytes, no null bytes
    if name.is_empty() || name.len() > 64 || name.contains('\0') {
        log::warn!("create_player: invalid name from {}", ctx.sender());
        return;
    }
    // Idempotent: skip if this identity already has a player row
    if ctx.db.player().identity().find(ctx.sender()).is_some() {
        log::info!("create_player: identity {} already exists, skipping", ctx.sender());
        return;
    }
    // Use the zone with the lowest id as the default spawn zone.
    // This avoids a hardcoded id=1 that breaks when zones are created with auto-increment ids.
    let Some(default_zone) = ctx.db.zone().iter().min_by_key(|z| z.id) else {
        log::warn!("create_player: no zones exist yet — create a zone first");
        return;
    };
    let default_zone_id = default_zone.id;
    let (spawn_x, spawn_y) = (
        default_zone.terrain_width  as f32 / 2.0,
        default_zone.terrain_height as f32 / 2.0,
    );

    let player = Player {
        id: 0,
        identity: ctx.sender(),
        name,
        zone_id: default_zone_id,
        position_x: spawn_x,
        position_y: spawn_y,
        health: 100,
        max_health: 100,
        mana: 100,
        max_mana: 100,
        is_dead: false,
    };
    ctx.db.player().insert(player);
    log::info!("Player created: {}", ctx.sender());
}

// Reducer to move a player
#[reducer]
pub fn move_player(ctx: &ReducerContext, new_x: f32, new_y: f32) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or_else(|| "Player not found".to_string())?;

    let zone = ctx.db.zone().id().find(&player.zone_id)
        .ok_or_else(|| "Zone not found".to_string())?;

    if !new_x.is_finite() || !new_y.is_finite() {
        return Err(format!("Invalid position ({}, {})", new_x, new_y));
    }

    if new_x < 0.0 || new_x > zone.terrain_width as f32
        || new_y < 0.0 || new_y > zone.terrain_height as f32
    {
        return Err(format!(
            "Position ({}, {}) out of zone bounds ({}x{})",
            new_x, new_y, zone.terrain_width, zone.terrain_height
        ));
    }
    ctx.db.player().id().update(Player {
        position_x: new_x,
        position_y: new_y,
        ..player
    });
    log::info!("Player moved to ({}, {})", new_x, new_y);
    Ok(())
}

// Reducer to create a zone and initialise flat terrain chunks
#[reducer]
pub fn create_zone(
    ctx: &ReducerContext,
    name: String,
    terrain_width: u32,
    terrain_height: u32,
    water_level: f32,
) {
    // Input validation
    const MAX_TERRAIN_DIM: u32 = 512;
    const MAX_NAME_LEN: usize = 128;
    if name.is_empty() || name.len() > MAX_NAME_LEN || name.contains('\0') {
        return;
    }
    if terrain_width == 0 || terrain_width > MAX_TERRAIN_DIM {
        return;
    }
    if terrain_height == 0 || terrain_height > MAX_TERRAIN_DIM {
        return;
    }
    if !water_level.is_finite() || water_level < 0.0 || water_level > terrain_height as f32 {
        return;
    }
    let zone = Zone {
        id: 0,
        name: name.clone(),
        terrain_width,
        terrain_height,
        water_level,
        mood_preset_id: 0,
    };
    let zone_row = ctx.db.zone().insert(zone);
    let zone_id = zone_row.id;

    // Default weather: clear skies
    ctx.db.weather_state().insert(WeatherState {
        zone_id,
        kind: WeatherKind::Clear,
        intensity: 0.0,
        started_at: ctx.timestamp,
    });

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

/// Admin: delete a zone and all its dependent data.
/// Rejects if any player is currently in the zone — caller must move/disconnect them first.
/// Cascades: terrain_chunks, weather_state, entity_instances, item_drops,
/// enemies (and their pending respawn ticks), spawn_points, portals (both directions).
#[reducer]
pub fn delete_zone(ctx: &ReducerContext, zone_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;

    // Refuse if any player is in this zone — orphaning them mid-session would corrupt state.
    if ctx.db.player().zone_id().filter(&zone_id).next().is_some() {
        return Err(format!("Zone {} has players in it; move them out first", zone_id));
    }

    // Collect every dependent id, then delete. Two-pass avoids iterator invalidation.
    let terrain_ids: Vec<u64> = ctx.db.terrain_chunk().zone_id().filter(&zone_id).map(|c| c.id).collect();
    for id in terrain_ids { ctx.db.terrain_chunk().id().delete(&id); }

    let entity_ids: Vec<u64> = ctx.db.entity_instance().iter()
        .filter(|e| e.zone_id == zone_id).map(|e| e.id).collect();
    for id in entity_ids { ctx.db.entity_instance().id().delete(&id); }

    let drop_ids: Vec<u64> = ctx.db.item_drop().zone_id().filter(&zone_id).map(|d| d.id).collect();
    for id in drop_ids { ctx.db.item_drop().id().delete(&id); }

    // Enemies + their pending respawn ticks
    let enemy_ids: Vec<u64> = ctx.db.enemy().zone_id().filter(&zone_id).map(|e| e.id).collect();
    let stale_tick_ids: Vec<u64> = ctx.db.enemy_respawn_tick().iter()
        .filter(|t| enemy_ids.contains(&t.enemy_id))
        .map(|t| t.scheduled_id)
        .collect();
    for tid in stale_tick_ids { ctx.db.enemy_respawn_tick().scheduled_id().delete(&tid); }
    for id in enemy_ids { ctx.db.enemy().id().delete(&id); }

    let spawn_ids: Vec<u64> = ctx.db.spawn_point().zone_id().filter(&zone_id).map(|s| s.id).collect();
    for id in spawn_ids { ctx.db.spawn_point().id().delete(&id); }

    // Portals where this zone is either source or destination
    let mut portal_ids: Vec<u64> = ctx.db.portal().source_zone_id().filter(&zone_id).map(|p| p.id).collect();
    portal_ids.extend(ctx.db.portal().dest_zone_id().filter(&zone_id).map(|p| p.id));
    portal_ids.sort_unstable();
    portal_ids.dedup();
    for id in portal_ids { ctx.db.portal().id().delete(&id); }

    ctx.db.weather_state().zone_id().delete(zone_id);
    ctx.db.zone().id().delete(&zone_id);

    log::info!("delete_zone: zone={} cascade-deleted", zone_id);
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
    if entity_type.is_empty() {
        return Err("entity_type cannot be empty".to_string());
    }
    if prefab_name.len() > 128 || entity_type.len() > 64 {
        return Err("prefab_name or entity_type exceeds maximum length".to_string());
    }

    // Validate zone exists and position is in bounds
    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;

    if !x.is_finite() || !y.is_finite() || !elevation.is_finite() {
        return Err("Non-finite position values".to_string());
    }
    if x < 0.0 || x > zone.terrain_width as f32 || y < 0.0 || y > zone.terrain_height as f32 {
        return Err(format!(
            "Position ({}, {}) out of zone bounds ({}x{})",
            x, y, zone.terrain_width, zone.terrain_height
        ));
    }
    const MAX_ELEVATION: f32 = 200.0;
    if elevation < -10.0 || elevation > MAX_ELEVATION {
        return Err(format!("Elevation {} out of range [-10, {}]", elevation, MAX_ELEVATION));
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


