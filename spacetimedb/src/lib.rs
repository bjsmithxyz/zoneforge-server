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

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum AiState {
    Idle,
    Chase,
    Attack,
}

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum EnemyType {
    Melee,
    Ranged,
    Caster,
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

#[table(accessor = mana_regen_tick, scheduled(tick_mana_regen))]
pub struct ManaRegenTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
}

// Defines an enemy archetype — shared stats referenced by all instances.
// Accessor matches autogen name "enemy_def".
#[table(accessor = enemy_def, public)]
pub struct EnemyDefinition {
    #[primary_key]
    #[auto_inc]
    pub id:              u64,
    pub name:            String,
    pub enemy_type:      EnemyType,
    pub prefab_name:     String,
    pub max_health:      i32,
    pub damage:          i32,
    pub aggro_range:     f32,
    pub attack_range:    f32,
    pub attack_speed_ms: u64,
    pub move_speed:      f32,
}

// Marks a location in a zone where enemies of a given def spawn automatically.
#[table(accessor = spawn_point, public)]
pub struct SpawnPoint {
    #[primary_key]
    #[auto_inc]
    pub id:               u64,
    #[index(btree)]
    pub zone_id:          u64,
    pub x:                f32,
    pub y:                f32,
    pub enemy_def_id:     u64,
    pub max_count:        u32,
    pub respawn_delay_s:  u32,
}

// One row per live (or recently dead) enemy instance.
#[table(accessor = enemy, public)]
pub struct Enemy {
    #[primary_key]
    #[auto_inc]
    pub id:               u64,
    #[index(btree)]
    pub zone_id:          u64,
    pub spawn_point_id:   Option<u64>,
    pub enemy_def_id:     u64,
    pub position_x:       f32,
    pub position_y:       f32,
    pub home_x:           f32,
    pub home_y:           f32,
    pub health:           i32,
    pub ai_state:         AiState,
    pub target_player_id: Option<u64>,
    pub last_attack_us:   u64,
    pub is_dead:          bool,
}

// One row per zone portal connection.
#[table(accessor = portal, public)]
pub struct Portal {
    #[primary_key]
    #[auto_inc]
    pub id:             u64,
    #[index(btree)]
    pub source_zone_id: u64,
    #[index(btree)]
    pub dest_zone_id:   u64,
    pub source_x:       f32,  // portal mouth position in source zone
    pub source_y:       f32,
    pub dest_spawn_x:   f32,  // player arrival + reverse exit point in dest zone
    pub dest_spawn_y:   f32,
    pub bidirectional:  bool,
    pub label:          String,  // e.g. "To Village" — optional display name
}

// Scheduled once per dead enemy to respawn it after respawn_delay_s.
#[table(accessor = enemy_respawn_tick, scheduled(tick_enemy_respawn))]
pub struct EnemyRespawnTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub enemy_id:     u64,
}

// Global AI tick — runs every 500ms to drive the enemy state machine.
#[table(accessor = ai_tick, scheduled(tick_ai))]
pub struct AiTick {
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

#[reducer]
pub fn tick_mana_regen(ctx: &ReducerContext, _tick: ManaRegenTick) {
    // Restore 10 mana every 2 seconds to all living players
    let players: Vec<Player> = ctx.db.player().iter().collect();
    for player in players {
        if player.is_dead || player.mana >= player.max_mana { continue; }
        let new_mana = (player.mana + 10).min(player.max_mana);
        ctx.db.player().id().update(Player { mana: new_mana, ..player });
    }
    // Re-schedule
    ctx.db.mana_regen_tick().insert(ManaRegenTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(2)
        ),
    });
}

/// Ensures all self-scheduling ticks are running after a hot-publish (init only runs on fresh databases).
#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
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
}

#[reducer]
pub fn tick_enemy_respawn(ctx: &ReducerContext, tick: EnemyRespawnTick) {
    let Some(enemy) = ctx.db.enemy().id().find(&tick.enemy_id) else { return; };
    if !enemy.is_dead { return; }  // Already revived

    let def = ctx.db.enemy_def().id().find(&enemy.enemy_def_id);
    let max_health = def.map(|d| d.max_health).unwrap_or(100);

    ctx.db.enemy().id().update(Enemy {
        health: max_health,
        is_dead: false,
        ai_state: AiState::Idle,
        target_player_id: None,
        position_x: enemy.home_x,
        position_y: enemy.home_y,
        ..enemy
    });
    log::info!("tick_enemy_respawn: enemy={} respawned", tick.enemy_id);
}

/// Drives the enemy AI state machine every 500ms.
#[reducer]
pub fn tick_ai(ctx: &ReducerContext, _tick: AiTick) {
    let now_us = ctx.timestamp
        .to_duration_since_unix_epoch()
        .unwrap_or_default()
        .as_micros() as u64;

    let enemies: Vec<Enemy> = ctx.db.enemy().iter().filter(|e| !e.is_dead).collect();

    for enemy in enemies {
        let Some(def) = ctx.db.enemy_def().id().find(&enemy.enemy_def_id) else { continue; };

        let updated = match enemy.ai_state.clone() {
            AiState::Idle => {
                let target = ctx.db.player().iter()
                    .filter(|p| !p.is_dead && p.zone_id == enemy.zone_id)
                    .min_by(|a, b| {
                        let da = dist_sq(a.position_x, a.position_y, enemy.position_x, enemy.position_y);
                        let db = dist_sq(b.position_x, b.position_y, enemy.position_x, enemy.position_y);
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });
                if let Some(p) = target {
                    let d2 = dist_sq(p.position_x, p.position_y, enemy.position_x, enemy.position_y);
                    if d2 <= def.aggro_range * def.aggro_range {
                        Enemy { ai_state: AiState::Chase, target_player_id: Some(p.id), ..enemy }
                    } else {
                        enemy
                    }
                } else {
                    enemy
                }
            },
            AiState::Chase => {
                let target_id = match enemy.target_player_id {
                    Some(id) => id,
                    None => {
                        let e = return_idle(&enemy);
                        ctx.db.enemy().id().update(e);
                        continue;
                    }
                };
                let Some(player) = ctx.db.player().id().find(&target_id) else {
                    let e = return_idle(&enemy);
                    ctx.db.enemy().id().update(e);
                    continue;
                };
                if player.is_dead {
                    let e = return_idle(&enemy);
                    ctx.db.enemy().id().update(e);
                    continue;
                }
                let d2 = dist_sq(player.position_x, player.position_y, enemy.position_x, enemy.position_y);
                if d2 > def.aggro_range * def.aggro_range * 2.25 {
                    let e = return_idle(&enemy);
                    ctx.db.enemy().id().update(e);
                    continue;
                }
                if d2 <= def.attack_range * def.attack_range {
                    Enemy { ai_state: AiState::Attack, ..enemy }
                } else {
                    let step = def.move_speed * 0.5;
                    let (nx, ny) = step_toward(
                        enemy.position_x, enemy.position_y,
                        player.position_x, player.position_y,
                        step,
                    );
                    Enemy { position_x: nx, position_y: ny, ..enemy }
                }
            },
            AiState::Attack => {
                let target_id = match enemy.target_player_id {
                    Some(id) => id,
                    None => {
                        let e = return_idle(&enemy);
                        ctx.db.enemy().id().update(e);
                        continue;
                    }
                };
                let Some(player) = ctx.db.player().id().find(&target_id) else {
                    let e = return_idle(&enemy);
                    ctx.db.enemy().id().update(e);
                    continue;
                };
                if player.is_dead {
                    let e = return_idle(&enemy);
                    ctx.db.enemy().id().update(e);
                    continue;
                }
                let d2 = dist_sq(player.position_x, player.position_y, enemy.position_x, enemy.position_y);
                if d2 > def.attack_range * def.attack_range {
                    Enemy { ai_state: AiState::Chase, ..enemy }
                } else {
                    let attack_interval_us = def.attack_speed_ms as u64 * 1000;
                    if now_us.saturating_sub(enemy.last_attack_us) >= attack_interval_us {
                        // attacker_id = enemy.id (enemy IDs share the u64 attacker_id column with player IDs)
                        apply_damage(ctx, target_id, enemy.id, 0, def.damage);
                        Enemy { last_attack_us: now_us, ..enemy }
                    } else {
                        enemy
                    }
                }
            },
        };
        ctx.db.enemy().id().update(updated);
    }

    // Re-schedule next tick
    ctx.db.ai_tick().insert(AiTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_millis(500)
        ),
    });
}

fn return_idle(enemy: &Enemy) -> Enemy {
    Enemy {
        id: enemy.id,
        zone_id: enemy.zone_id,
        spawn_point_id: enemy.spawn_point_id,
        enemy_def_id: enemy.enemy_def_id,
        position_x: enemy.home_x,
        position_y: enemy.home_y,
        home_x: enemy.home_x,
        home_y: enemy.home_y,
        health: enemy.health,
        ai_state: AiState::Idle,
        target_player_id: None,
        last_attack_us: enemy.last_attack_us,
        is_dead: enemy.is_dead,
    }
}

fn dist_sq(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

fn step_toward(from_x: f32, from_y: f32, to_x: f32, to_y: f32, step: f32) -> (f32, f32) {
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

    // Guard against pathological damage values that could overflow i32 arithmetic
    const MAX_ABILITY_DAMAGE: i32 = 10_000;
    if ability.damage.abs() > MAX_ABILITY_DAMAGE {
        log::error!("Ability {} has invalid damage value {}", ability_id, ability.damage);
        return Err("Invalid ability configuration".to_string());
    }

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
    if !is_admin(ctx) {
        log::warn!("create_zone: unauthorized caller {}", ctx.sender());
        return;
    }
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
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    if height_data.len() != 4096 {
        return Err(format!("height_data must be 4096 bytes, got {}", height_data.len()));
    }
    if splat_data.len() != 4096 {
        return Err(format!("splat_data must be 4096 bytes, got {}", splat_data.len()));
    }

    // Validate height values: each group of 4 bytes is a little-endian f32.
    // Reject NaN and Infinity which would corrupt terrain rendering.
    for chunk in height_data.chunks_exact(4) {
        let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if !val.is_finite() {
            return Err(format!("height_data contains non-finite float value: {}", val));
        }
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
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
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

#[reducer]
pub fn create_enemy_def(
    ctx: &ReducerContext,
    name: String,
    enemy_type: EnemyType,
    prefab_name: String,
    max_health: i32,
    damage: i32,
    aggro_range: f32,
    attack_range: f32,
    attack_speed_ms: u64,
    move_speed: f32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    if name.is_empty() || name.len() > 128 || name.contains('\0') {
        return Err("Invalid enemy def name".to_string());
    }
    if prefab_name.is_empty() || prefab_name.len() > 128 {
        return Err("Invalid prefab_name".to_string());
    }
    if max_health <= 0 || max_health > 100_000 {
        return Err(format!("max_health {} out of range [1, 100000]", max_health));
    }
    if damage < 0 || damage > 10_000 {
        return Err(format!("damage {} out of range [0, 10000]", damage));
    }
    if !aggro_range.is_finite() || aggro_range < 0.0 || aggro_range > 200.0 {
        return Err("aggro_range out of range [0, 200]".to_string());
    }
    if !attack_range.is_finite() || attack_range < 0.0 || attack_range > aggro_range {
        return Err("attack_range must be in [0, aggro_range]".to_string());
    }
    if attack_speed_ms == 0 || attack_speed_ms > 60_000 {
        return Err("attack_speed_ms out of range [1, 60000]".to_string());
    }
    if !move_speed.is_finite() || move_speed < 0.0 || move_speed > 100.0 {
        return Err("move_speed out of range [0, 100]".to_string());
    }
    ctx.db.enemy_def().insert(EnemyDefinition {
        id: 0,
        name,
        enemy_type,
        prefab_name,
        max_health,
        damage,
        aggro_range,
        attack_range,
        attack_speed_ms,
        move_speed,
    });
    Ok(())
}

#[reducer]
pub fn delete_enemy_def(ctx: &ReducerContext, def_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.enemy_def().id().find(&def_id)
        .ok_or_else(|| format!("EnemyDef {} not found", def_id))?;
    ctx.db.enemy_def().id().delete(&def_id);
    Ok(())
}

#[reducer]
pub fn create_spawn_point(
    ctx: &ReducerContext,
    zone_id: u64,
    x: f32,
    y: f32,
    enemy_def_id: u64,
    max_count: u32,
    respawn_delay_s: u32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;
    ctx.db.enemy_def().id().find(&enemy_def_id)
        .ok_or_else(|| format!("EnemyDef {} not found", enemy_def_id))?;
    if !x.is_finite() || !y.is_finite()
        || x < 0.0 || x > zone.terrain_width as f32
        || y < 0.0 || y > zone.terrain_height as f32
    {
        return Err("Spawn point position out of zone bounds".to_string());
    }
    if max_count == 0 || max_count > 100 {
        return Err("max_count out of range [1, 100]".to_string());
    }
    if respawn_delay_s > 3600 {
        return Err("respawn_delay_s must be <= 3600".to_string());
    }
    ctx.db.spawn_point().insert(SpawnPoint {
        id: 0,
        zone_id,
        x,
        y,
        enemy_def_id,
        max_count,
        respawn_delay_s,
    });
    Ok(())
}

#[reducer]
pub fn delete_spawn_point(ctx: &ReducerContext, spawn_point_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.spawn_point().id().find(&spawn_point_id)
        .ok_or_else(|| format!("SpawnPoint {} not found", spawn_point_id))?;
    ctx.db.spawn_point().id().delete(&spawn_point_id);
    Ok(())
}

#[reducer]
pub fn spawn_enemy_manual(
    ctx: &ReducerContext,
    zone_id: u64,
    x: f32,
    y: f32,
    enemy_def_id: u64,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;
    let def = ctx.db.enemy_def().id().find(&enemy_def_id)
        .ok_or_else(|| format!("EnemyDef {} not found", enemy_def_id))?;
    if !x.is_finite() || !y.is_finite()
        || x < 0.0 || x > zone.terrain_width as f32
        || y < 0.0 || y > zone.terrain_height as f32
    {
        return Err("Spawn position out of zone bounds".to_string());
    }
    ctx.db.enemy().insert(Enemy {
        id: 0,
        zone_id,
        spawn_point_id: None,
        enemy_def_id,
        position_x: x,
        position_y: y,
        home_x: x,
        home_y: y,
        health: def.max_health,
        ai_state: AiState::Idle,
        target_player_id: None,
        last_attack_us: 0,
        is_dead: false,
    });
    Ok(())
}

#[reducer]
pub fn despawn_enemy(ctx: &ReducerContext, enemy_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.enemy().id().find(&enemy_id)
        .ok_or_else(|| format!("Enemy {} not found", enemy_id))?;
    ctx.db.enemy().id().delete(&enemy_id);
    Ok(())
}

/// Player uses an ability to attack an enemy. Called from CombatInputHandler.
#[reducer]
pub fn attack_enemy(
    ctx: &ReducerContext,
    ability_id: u64,
    enemy_id: u64,
) -> Result<(), String> {
    // 1. Caller must exist and not be dead
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Cannot use ability while dead".to_string());
    }

    // 2. Ability must exist and be non-self-cast
    let ability = ctx.db.ability().id().find(&ability_id)
        .ok_or("Ability not found")?;
    if ability.ability_type == AbilityType::SelfCast {
        return Err("Self-cast abilities cannot target enemies".to_string());
    }

    // 3. Target enemy must exist and not be dead
    let enemy = ctx.db.enemy().id().find(&enemy_id)
        .ok_or("Enemy not found")?;
    if enemy.is_dead {
        return Err("Enemy is already dead".to_string());
    }

    // 4. Range check
    let dx = player.position_x - enemy.position_x;
    let dz = player.position_y - enemy.position_y;
    let dist_sq = dx * dx + dz * dz;
    if dist_sq > ability.range * ability.range {
        return Err(format!(
            "Enemy out of range (dist={:.1}, range={:.1})",
            dist_sq.sqrt(), ability.range
        ));
    }

    // 5. Cooldown check
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

    // 6. Mana check
    if player.mana < ability.mana_cost {
        return Err(format!("Insufficient mana ({}/{})", player.mana, ability.mana_cost));
    }

    // 7. Damage bounds guard
    const MAX_ABILITY_DAMAGE: i32 = 10_000;
    if ability.damage.abs() > MAX_ABILITY_DAMAGE {
        log::error!("Ability {} has invalid damage value {}", ability_id, ability.damage);
        return Err("Invalid ability configuration".to_string());
    }

    // All checks passed — deduct mana and update cooldown
    let player_id = player.id;
    let new_mana = player.mana - ability.mana_cost;
    ctx.db.player().id().update(Player { mana: new_mana, ..player });

    let ready_at = ctx.timestamp + std::time::Duration::from_millis(ability.cooldown_ms);
    if let Some(existing_cd) = ctx.db.player_cooldown()
        .player_id()
        .filter(&player_id)
        .find(|cd| cd.ability_id == ability_id)
    {
        ctx.db.player_cooldown().id().update(PlayerCooldown { ready_at, ..existing_cd });
    } else {
        ctx.db.player_cooldown().insert(PlayerCooldown {
            id: 0,
            player_id,
            ability_id,
            ready_at,
        });
    }

    // Apply damage to enemy
    apply_damage_to_enemy(ctx, enemy_id, player_id, ability.damage);

    log::info!("attack_enemy: player={} ability={} enemy={}", player_id, ability_id, enemy_id);
    Ok(())
}

/// Applies `amount` damage to an enemy. Negative amount = heal.
fn apply_damage_to_enemy(
    ctx: &ReducerContext,
    enemy_id: u64,
    attacker_id: u64,
    amount: i32,
) {
    let Some(enemy) = ctx.db.enemy().id().find(&enemy_id) else { return; };
    if enemy.is_dead { return; }

    let def = ctx.db.enemy_def().id().find(&enemy.enemy_def_id);
    let max_health = def.map(|d| d.max_health).unwrap_or(enemy.health);

    let new_health = (enemy.health - amount).clamp(0, max_health);
    let is_dead = new_health == 0 && amount > 0;
    // Save spawn_point_id before enemy is moved into the update struct
    let spawn_point_id = enemy.spawn_point_id;

    ctx.db.enemy().id().update(Enemy {
        health: new_health,
        is_dead,
        ai_state: if is_dead { AiState::Idle } else { enemy.ai_state.clone() },
        target_player_id: if is_dead { None } else { enemy.target_player_id },
        ..enemy
    });

    if is_dead {
        log::info!("apply_damage_to_enemy: enemy={} killed by player={}", enemy_id, attacker_id);
        // Schedule respawn if this enemy belongs to a spawn point
        if let Some(sp_id) = spawn_point_id {
            if let Some(sp) = ctx.db.spawn_point().id().find(&sp_id) {
                ctx.db.enemy_respawn_tick().insert(EnemyRespawnTick {
                    scheduled_id: 0,
                    scheduled_at: ScheduleAt::Time(
                        ctx.timestamp + std::time::Duration::from_secs(sp.respawn_delay_s as u64)
                    ),
                    enemy_id,
                });
            }
        }
    }
}

/// Admin-only. Creates a portal between two zones.
/// Validates both zones exist and positions are within their bounds.
#[reducer]
pub fn create_portal(
    ctx: &ReducerContext,
    source_zone_id: u64,
    dest_zone_id: u64,
    source_x: f32,
    source_y: f32,
    dest_spawn_x: f32,
    dest_spawn_y: f32,
    bidirectional: bool,
    label: String,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    if !source_x.is_finite() || !source_y.is_finite()
        || !dest_spawn_x.is_finite() || !dest_spawn_y.is_finite() {
        return Err("Portal coordinates must be finite".to_string());
    }
    if label.len() > 64 || label.contains('\0') {
        return Err("label exceeds 64 bytes or contains invalid characters".to_string());
    }
    if source_zone_id == dest_zone_id {
        return Err("source and destination zones must differ".to_string());
    }
    let source_zone = ctx.db.zone().id().find(&source_zone_id)
        .ok_or_else(|| format!("Zone {} not found", source_zone_id))?;
    let dest_zone = ctx.db.zone().id().find(&dest_zone_id)
        .ok_or_else(|| format!("Zone {} not found", dest_zone_id))?;
    if source_x < 0.0 || source_x > source_zone.terrain_width as f32
        || source_y < 0.0 || source_y > source_zone.terrain_height as f32 {
        return Err(format!("source_x/y out of bounds for zone {}", source_zone_id));
    }
    if dest_spawn_x < 0.0 || dest_spawn_x > dest_zone.terrain_width as f32
        || dest_spawn_y < 0.0 || dest_spawn_y > dest_zone.terrain_height as f32 {
        return Err(format!("dest_spawn_x/y out of bounds for zone {}", dest_zone_id));
    }
    ctx.db.portal().insert(Portal {
        id: 0,
        source_zone_id,
        dest_zone_id,
        source_x,
        source_y,
        dest_spawn_x,
        dest_spawn_y,
        bidirectional,
        label,
    });
    log::info!("create_portal: {} -> {} at ({},{})", source_zone_id, dest_zone_id, source_x, source_y);
    Ok(())
}

/// Admin-only. Deletes a portal by ID.
#[reducer]
pub fn delete_portal(ctx: &ReducerContext, portal_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.portal().id().find(&portal_id)
        .ok_or_else(|| format!("Portal {} not found", portal_id))?;
    ctx.db.portal().id().delete(&portal_id);
    log::info!("delete_portal: {}", portal_id);
    Ok(())
}

/// Any player. Moves the player through a portal they are standing near.
/// Checks portals originating from the player's current zone (forward travel).
/// For bidirectional portals, also checks reverse travel from dest_spawn position.
/// Returns Err if no portal is within 2 world units of the player's position.
#[reducer]
pub fn enter_zone(ctx: &ReducerContext) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Dead players cannot use portals".to_string());
    }

    // Collect portal candidates before consuming `player` in the update call
    let zone_id = player.zone_id;
    let px = player.position_x;
    let py = player.position_y;

    // Forward: portals where this zone is the source
    for portal in ctx.db.portal().source_zone_id().filter(&zone_id) {
        if dist_sq(px, py, portal.source_x, portal.source_y) < 4.0 {
            let dest_zone_id = portal.dest_zone_id;
            ctx.db.player().id().update(Player {
                zone_id:    portal.dest_zone_id,
                position_x: portal.dest_spawn_x,
                position_y: portal.dest_spawn_y,
                ..player
            });
            log::info!("enter_zone: {:?} -> zone {}", ctx.sender(), dest_zone_id);
            return Ok(());
        }
    }

    // Reverse: bidirectional portals where this zone is the dest
    for portal in ctx.db.portal().dest_zone_id().filter(&zone_id) {
        if !portal.bidirectional { continue; }
        if dist_sq(px, py, portal.dest_spawn_x, portal.dest_spawn_y) < 4.0 {
            let src_zone_id = portal.source_zone_id;
            ctx.db.player().id().update(Player {
                zone_id:    portal.source_zone_id,
                position_x: portal.source_x,
                position_y: portal.source_y,
                ..player
            });
            log::info!("enter_zone: {:?} (reverse) -> zone {}", ctx.sender(), src_zone_id);
            return Ok(());
        }
    }

    Err("No portal within range".to_string())
}
