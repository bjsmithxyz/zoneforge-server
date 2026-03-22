use spacetimedb::{table, reducer, ReducerContext, Identity, Table, SpacetimeType, Timestamp, ScheduleAt};

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum AbilityType {
    MeleeAttack,
    Projectile,
    SelfCast,
}

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum StatusEffectType {
    Burn,
    Freeze,
    Stun,
    Poison,
}

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
    pub mana: i32,
    pub max_mana: i32,
    pub is_dead: bool,
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

#[table(accessor = ability, public)]
pub struct Ability {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub damage: i32,
    pub cooldown_ms: u64,
    pub mana_cost: i32,
    pub range: f32,
    pub ability_type: AbilityType,
}

// Surrogate id PK + btree index on player_id (no composite PK — SpacetimeDB 2.x limitation).
// Upsert via: player_cooldown().player_id().filter(&player_id).find(|cd| cd.ability_id == ability_id)
#[table(accessor = player_cooldown, public)]
pub struct PlayerCooldown {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub player_id: u64,
    pub ability_id: u64,
    pub ready_at: Timestamp,
}

#[table(accessor = status_effect, public)]
pub struct StatusEffect {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub target_id: u64,
    pub effect_type: StatusEffectType,
    pub expires_at: Timestamp,
    pub damage_per_tick: i32,
}

#[table(accessor = combat_log, public)]
pub struct CombatLog {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub timestamp: Timestamp,
    pub attacker_id: u64,
    pub target_id: u64,
    pub ability_id: u64,
    pub damage_dealt: i32,
    pub overkill: i32,
}

#[table(accessor = status_effect_tick, scheduled(tick_status_effects))]
pub struct StatusEffectTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
}

#[reducer]
pub fn tick_status_effects(ctx: &ReducerContext, _tick: StatusEffectTick) {
    let now_us = ctx.timestamp
        .to_duration_since_unix_epoch()
        .unwrap_or_default()
        .as_micros();

    // Collect all current effects (avoid borrow issues while deleting)
    let all_effects: Vec<StatusEffect> = ctx.db.status_effect().iter().collect();

    for effect in all_effects {
        let expires_us = effect.expires_at
            .to_duration_since_unix_epoch()
            .unwrap_or_default()
            .as_micros();

        if expires_us <= now_us {
            // Expired — remove it
            ctx.db.status_effect().id().delete(&effect.id);
        } else if matches!(effect.effect_type, StatusEffectType::Burn | StatusEffectType::Poison) {
            // Active DoT — apply tick damage (ability_id 0 = DoT, no ability row)
            apply_damage(ctx, effect.target_id, effect.target_id, 0, effect.damage_per_tick);
        }
        // Freeze/Stun: no damage tick — effect persists until expired (handled by expiry branch above)
    }

    // Re-schedule for next tick (self-recurring pattern)
    ctx.db.status_effect_tick().insert(StatusEffectTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(1)
        ),
    });
}

#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
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
}

fn apply_damage(
    ctx: &ReducerContext,
    target_id: u64,
    attacker_id: u64,
    ability_id: u64,
    amount: i32,
) {
    let Some(target) = ctx.db.player().id().find(&target_id) else {
        return;
    };
    // Skip if target is already dead (prevents DoT ticks from writing spurious log rows)
    if target.is_dead {
        return;
    }

    let new_health = (target.health - amount).clamp(0, target.max_health);
    let overkill = if amount > 0 && amount > target.health {
        amount - target.health
    } else {
        0
    };
    let new_is_dead = (new_health == 0 && amount > 0) || target.is_dead;

    ctx.db.player().id().update(Player {
        health: new_health,
        is_dead: new_is_dead,
        ..target
    });

    ctx.db.combat_log().insert(CombatLog {
        id: 0,
        timestamp: ctx.timestamp,
        attacker_id,
        target_id,
        ability_id,
        damage_dealt: amount,
        overkill,
    });

    log::info!(
        "apply_damage: target={} amount={} new_health={} dead={}",
        target_id, amount, new_health, new_is_dead
    );
}

#[reducer]
pub fn use_ability(
    ctx: &ReducerContext,
    ability_id: u64,
    target_id: u64,
) -> Result<(), String> {
    // 1. Caller must exist and not be dead
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Cannot use ability while dead".to_string());
    }

    // 2. Ability must exist
    let ability = ctx.db.ability().id().find(&ability_id)
        .ok_or("Ability not found")?;

    // 3. Self-cast: target must be the caller
    if ability.ability_type == AbilityType::SelfCast {
        if target_id != player.id {
            return Err("Self-cast ability must target self".to_string());
        }
    } else {
        // 4. Target must exist and not be dead
        let target = ctx.db.player().id().find(&target_id)
            .ok_or("Target not found")?;
        if target.is_dead {
            return Err("Target is already dead".to_string());
        }

        // 5. Range check (XZ distance); position_y is the horizontal Z axis (no position_z field)
        let dx = player.position_x - target.position_x;
        let dz = player.position_y - target.position_y;
        let dist_sq = dx * dx + dz * dz;
        if dist_sq > ability.range * ability.range {
            return Err(format!(
                "Target out of range (dist={:.1}, range={:.1})",
                dist_sq.sqrt(), ability.range
            ));
        }
    }

    // 6. Cooldown check
    let now_us = ctx.timestamp
        .to_duration_since_unix_epoch()
        .unwrap_or_default()
        .as_micros();
    let on_cooldown = ctx.db.player_cooldown()
        .player_id()
        .filter(&player.id)
        .any(|cd| {
            if cd.ability_id != ability_id { return false; }
            let ready_us = cd.ready_at
                .to_duration_since_unix_epoch()
                .unwrap_or_default()
                .as_micros();
            ready_us > now_us
        });
    if on_cooldown {
        return Err("Ability on cooldown".to_string());
    }

    // 7. Mana check
    if player.mana < ability.mana_cost {
        return Err(format!(
            "Insufficient mana ({}/{})", player.mana, ability.mana_cost
        ));
    }

    // All checks passed — save id before player is moved into the struct update
    let player_id = player.id;
    let new_mana = player.mana - ability.mana_cost;
    ctx.db.player().id().update(Player { mana: new_mana, ..player });
    // `player` is moved above; use `player_id` from here on

    // Upsert cooldown: find existing row for this player+ability, update or insert
    let ready_at = ctx.timestamp
        + std::time::Duration::from_millis(ability.cooldown_ms);
    if let Some(existing_cd) = ctx.db.player_cooldown()
        .player_id()
        .filter(&player_id)
        .find(|cd| cd.ability_id == ability_id)
    {
        ctx.db.player_cooldown().id().update(PlayerCooldown {
            ready_at,
            ..existing_cd
        });
    } else {
        ctx.db.player_cooldown().insert(PlayerCooldown {
            id: 0,
            player_id,
            ability_id,
            ready_at,
        });
    }

    // Apply effect — negative damage = heal (see apply_damage for how negative amount is handled)
    apply_damage(ctx, target_id, player_id, ability_id, ability.damage);

    log::info!(
        "use_ability: player={} ability={} target={}",
        player_id, ability_id, target_id
    );
    Ok(())
}

#[reducer]
pub fn respawn(ctx: &ReducerContext) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if !player.is_dead {
        return Err("Player is not dead".to_string());
    }

    let zone = ctx.db.zone().id().find(&player.zone_id)
        .ok_or("Zone not found")?;

    let spawn_x = zone.terrain_width as f32 / 2.0;
    let spawn_y = zone.terrain_height as f32 / 2.0;

    // Save player_id before player is moved into the struct update
    let player_id = player.id;
    ctx.db.player().id().update(Player {
        health: player.max_health,
        mana: player.max_mana,
        is_dead: false,
        position_x: spawn_x,
        position_y: spawn_y,
        ..player
    });

    // Remove all active status effects for this player
    let effect_ids: Vec<u64> = ctx.db.status_effect()
        .target_id()
        .filter(&player_id)
        .map(|e| e.id)
        .collect();
    for effect_id in effect_ids {
        ctx.db.status_effect().id().delete(&effect_id);
    }

    log::info!("respawn: player={} at ({}, {})", player_id, spawn_x, spawn_y);
    Ok(())
}

// Reducer to create a new player
#[reducer]
pub fn create_player(ctx: &ReducerContext, name: String) {
    // Idempotent: skip if this identity already has a player row
    if ctx.db.player().identity().find(ctx.sender()).is_some() {
        log::info!("create_player: identity {} already exists, skipping", ctx.sender());
        return;
    }
    let (spawn_x, spawn_y) = ctx.db.zone().id().find(&1u64)
        .map(|z| (z.terrain_width as f32 / 2.0, z.terrain_height as f32 / 2.0))
        .unwrap_or((32.0, 32.0));

    let player = Player {
        id: 0,
        identity: ctx.sender(),
        name,
        zone_id: 1,
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

    let clamped_x = new_x.clamp(0.0, zone.terrain_width as f32);
    let clamped_y = new_y.clamp(0.0, zone.terrain_height as f32);

    if clamped_x != new_x || clamped_y != new_y {
        log::warn!(
            "move_player: position ({}, {}) clamped to ({}, {})",
            new_x, new_y, clamped_x, clamped_y
        );
    }

    ctx.db.player().id().update(Player {
        position_x: clamped_x,
        position_y: clamped_y,
        ..player
    });
    log::info!("Player moved to ({}, {})", clamped_x, clamped_y);
    Ok(())
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
