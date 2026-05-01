use spacetimedb::{table, reducer, ReducerContext, SpacetimeType, Table, Timestamp, ScheduleAt};

use crate::{AiState, Enemy, Player, EnemyRespawnTick};
use crate::{player as _, zone as _};
use crate::enemy::{enemy as _, enemy_def as _, spawn_point as _, enemy_respawn_tick as _};
use crate::loot::spawn_loot_drops;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum AbilityType {
    MeleeAttack,
    Projectile,
    SelfCast,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum StatusEffectType {
    Burn,
    Freeze,
    Stun,
    Poison,
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

#[table(accessor = combat_log_prune_tick, scheduled(prune_combat_log))]
pub struct CombatLogPruneTick {
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

    let all_effects: Vec<StatusEffect> = ctx.db.status_effect().iter().collect();

    for effect in all_effects {
        let expires_us = effect.expires_at
            .to_duration_since_unix_epoch()
            .unwrap_or_default()
            .as_micros();

        if expires_us <= now_us {
            ctx.db.status_effect().id().delete(&effect.id);
        } else if matches!(effect.effect_type, StatusEffectType::Burn | StatusEffectType::Poison) {
            apply_damage(ctx, effect.target_id, effect.target_id, 0, effect.damage_per_tick);
        }
    }

    ctx.db.status_effect_tick().insert(StatusEffectTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(1)
        ),
    });
}

/// Caps `combat_log` at the latest `KEEP` rows. Without this, every damage tick
/// adds a row forever — bandwidth + memory grow unbounded since clients subscribe
/// to the full table. Runs every 60s.
#[reducer]
pub fn prune_combat_log(ctx: &ReducerContext, _tick: CombatLogPruneTick) {
    const KEEP: usize = 1000;
    let mut ids: Vec<u64> = ctx.db.combat_log().iter().map(|l| l.id).collect();
    if ids.len() > KEEP {
        ids.sort_unstable();
        let to_delete = ids.len() - KEEP;
        for id in ids.into_iter().take(to_delete) {
            ctx.db.combat_log().id().delete(&id);
        }
    }
    ctx.db.combat_log_prune_tick().insert(CombatLogPruneTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(60)
        ),
    });
}

#[reducer]
pub fn tick_mana_regen(ctx: &ReducerContext, _tick: ManaRegenTick) {
    // Restore 10 mana every 2 seconds. Collect ids only — avoids cloning the
    // full Player row for every player on every tick. Re-fetch each before update.
    let ids: Vec<u64> = ctx.db.player().iter()
        .filter(|p| !p.is_dead && p.mana < p.max_mana)
        .map(|p| p.id)
        .collect();
    for id in ids {
        if let Some(player) = ctx.db.player().id().find(&id) {
            let new_mana = (player.mana + 10).min(player.max_mana);
            ctx.db.player().id().update(Player { mana: new_mana, ..player });
        }
    }
    ctx.db.mana_regen_tick().insert(ManaRegenTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(2)
        ),
    });
}

pub(crate) fn apply_damage(
    ctx: &ReducerContext,
    target_id: u64,
    attacker_id: u64,
    ability_id: u64,
    amount: i32,
) {
    let Some(target) = ctx.db.player().id().find(&target_id) else {
        return;
    };
    if target.is_dead {
        return;
    }

    let new_health = (target.health - amount).clamp(0, target.max_health);
    let overkill = if amount > 0 && amount > target.health {
        amount - target.health
    } else {
        0
    };
    let new_is_dead = new_health == 0 && amount > 0;

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

    spacetimedb::log::info!(
        "apply_damage: target={} amount={} new_health={} dead={}",
        target_id, amount, new_health, new_is_dead
    );
}

pub(crate) fn apply_damage_to_enemy(
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
    let spawn_point_id = enemy.spawn_point_id;

    ctx.db.enemy().id().update(Enemy {
        health: new_health,
        is_dead,
        ai_state: if is_dead { AiState::Idle } else { enemy.ai_state },
        target_player_id: if is_dead { None } else { enemy.target_player_id },
        ..enemy
    });

    if is_dead {
        spacetimedb::log::info!("apply_damage_to_enemy: enemy={} killed by player={}", enemy_id, attacker_id);
        if let Some(dead_enemy) = ctx.db.enemy().id().find(&enemy_id) {
            spawn_loot_drops(ctx, &dead_enemy);
        }
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

#[reducer]
pub fn use_ability(
    ctx: &ReducerContext,
    ability_id: u64,
    target_id: u64,
) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Cannot use ability while dead".to_string());
    }

    let ability = ctx.db.ability().id().find(&ability_id)
        .ok_or("Ability not found")?;

    const MAX_ABILITY_DAMAGE: i32 = 10_000;
    if ability.damage.abs() > MAX_ABILITY_DAMAGE {
        spacetimedb::log::error!("Ability {} has invalid damage value {}", ability_id, ability.damage);
        return Err("Invalid ability configuration".to_string());
    }

    if ability.ability_type == AbilityType::SelfCast {
        if target_id != player.id {
            return Err("Self-cast ability must target self".to_string());
        }
    } else {
        let target = ctx.db.player().id().find(&target_id)
            .ok_or("Target not found")?;
        if target.is_dead {
            return Err("Target is already dead".to_string());
        }

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

    if player.mana < ability.mana_cost {
        return Err(format!(
            "Insufficient mana ({}/{})", player.mana, ability.mana_cost
        ));
    }

    let player_id = player.id;
    let new_mana = player.mana - ability.mana_cost;
    ctx.db.player().id().update(Player { mana: new_mana, ..player });

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

    apply_damage(ctx, target_id, player_id, ability_id, ability.damage);

    spacetimedb::log::info!(
        "use_ability: player={} ability={} target={}",
        player_id, ability_id, target_id
    );
    Ok(())
}

#[reducer]
pub fn attack_enemy(
    ctx: &ReducerContext,
    ability_id: u64,
    enemy_id: u64,
) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Cannot use ability while dead".to_string());
    }

    let ability = ctx.db.ability().id().find(&ability_id)
        .ok_or("Ability not found")?;
    if ability.ability_type == AbilityType::SelfCast {
        return Err("Self-cast abilities cannot target enemies".to_string());
    }

    let enemy = ctx.db.enemy().id().find(&enemy_id)
        .ok_or("Enemy not found")?;
    if enemy.is_dead {
        return Err("Enemy is already dead".to_string());
    }

    let dx = player.position_x - enemy.position_x;
    let dz = player.position_y - enemy.position_y;
    let dist_sq = dx * dx + dz * dz;
    if dist_sq > ability.range * ability.range {
        return Err(format!(
            "Enemy out of range (dist={:.1}, range={:.1})",
            dist_sq.sqrt(), ability.range
        ));
    }

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

    if player.mana < ability.mana_cost {
        return Err(format!("Insufficient mana ({}/{})", player.mana, ability.mana_cost));
    }

    const MAX_ABILITY_DAMAGE: i32 = 10_000;
    if ability.damage.abs() > MAX_ABILITY_DAMAGE {
        spacetimedb::log::error!("Ability {} has invalid damage value {}", ability_id, ability.damage);
        return Err("Invalid ability configuration".to_string());
    }

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

    apply_damage_to_enemy(ctx, enemy_id, player_id, ability.damage);

    spacetimedb::log::info!("attack_enemy: player={} ability={} enemy={}", player_id, ability_id, enemy_id);
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

    let player_id = player.id;
    ctx.db.player().id().update(Player {
        health: player.max_health,
        mana: player.max_mana,
        is_dead: false,
        position_x: spawn_x,
        position_y: spawn_y,
        ..player
    });

    let effect_ids: Vec<u64> = ctx.db.status_effect()
        .target_id()
        .filter(&player_id)
        .map(|e| e.id)
        .collect();
    for effect_id in effect_ids {
        ctx.db.status_effect().id().delete(&effect_id);
    }

    spacetimedb::log::info!("respawn: player={} at ({}, {})", player_id, spawn_x, spawn_y);
    Ok(())
}
