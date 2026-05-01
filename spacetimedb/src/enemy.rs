use spacetimedb::{table, reducer, ReducerContext, SpacetimeType, Table, ScheduleAt};

use crate::{dist_sq, step_toward, is_admin};
use crate::combat::apply_damage;
use crate::{player as _, zone as _};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum AiState {
    Idle,
    Chase,
    Attack,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum EnemyType {
    Melee,
    Ranged,
    Caster,
}

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

#[table(accessor = enemy_respawn_tick, scheduled(tick_enemy_respawn))]
pub struct EnemyRespawnTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
    pub enemy_id:     u64,
}

#[table(accessor = ai_tick, scheduled(tick_ai))]
pub struct AiTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
}

#[reducer]
pub fn tick_enemy_respawn(ctx: &ReducerContext, tick: EnemyRespawnTick) {
    let Some(enemy) = ctx.db.enemy().id().find(&tick.enemy_id) else { return; };
    if !enemy.is_dead { return; }

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
    spacetimedb::log::info!("tick_enemy_respawn: enemy={} respawned", tick.enemy_id);
}

#[reducer]
pub fn tick_ai(ctx: &ReducerContext, _tick: AiTick) {
    let now_us = ctx.timestamp
        .to_duration_since_unix_epoch()
        .unwrap_or_default()
        .as_micros() as u64;

    let enemies: Vec<Enemy> = ctx.db.enemy().iter().filter(|e| !e.is_dead).collect();

    for enemy in enemies {
        let Some(def) = ctx.db.enemy_def().id().find(&enemy.enemy_def_id) else { continue; };

        let updated = match enemy.ai_state {
            AiState::Idle => {
                let target = ctx.db.player().zone_id().filter(&enemy.zone_id)
                    .filter(|p| !p.is_dead)
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
                    let attack_interval_us = def.attack_speed_ms * 1000;
                    if now_us.saturating_sub(enemy.last_attack_us) >= attack_interval_us {
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

    // Cascade: orphan any pending respawn ticks for enemies tied to this spawn point.
    let orphaned_enemy_ids: Vec<u64> = ctx.db.enemy().iter()
        .filter(|e| e.spawn_point_id == Some(spawn_point_id))
        .map(|e| e.id)
        .collect();
    let stale_tick_ids: Vec<u64> = ctx.db.enemy_respawn_tick().iter()
        .filter(|t| orphaned_enemy_ids.contains(&t.enemy_id))
        .map(|t| t.scheduled_id)
        .collect();
    for tid in stale_tick_ids {
        ctx.db.enemy_respawn_tick().scheduled_id().delete(&tid);
    }

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
pub fn update_ai_state(
    ctx: &ReducerContext,
    enemy_id: u64,
    new_state: AiState,
    target_player_id: Option<u64>,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    let enemy = ctx.db.enemy().id().find(&enemy_id)
        .ok_or_else(|| format!("Enemy {} not found", enemy_id))?;
    if enemy.is_dead {
        return Err("Cannot update AI state of dead enemy".to_string());
    }
    spacetimedb::log::info!("update_ai_state: enemy={} state={:?} target={:?}", enemy_id, new_state, target_player_id);
    ctx.db.enemy().id().update(Enemy {
        ai_state: new_state,
        target_player_id,
        ..enemy
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
