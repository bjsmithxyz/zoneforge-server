use spacetimedb::{table, reducer, ReducerContext, SpacetimeType, Table, Timestamp, ScheduleAt};

use crate::is_admin;
use crate::zone as _;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum WeatherKind {
    Clear,
    Rain,
    Storm,
    Fog,
    Snow,
}

#[table(accessor = weather_state, public)]
pub struct WeatherState {
    #[primary_key]
    pub zone_id: u64,
    pub kind: WeatherKind,
    pub intensity: f32,
    pub started_at: Timestamp,
}

#[table(accessor = world_clock, public)]
pub struct WorldClock {
    #[primary_key]
    pub id: u8,
    pub minutes_of_day: u16,
    pub last_tick: Timestamp,
}

#[table(accessor = world_clock_tick, scheduled(tick_world_time))]
pub struct WorldClockTick {
    #[primary_key]
    #[auto_inc]
    pub scheduled_id: u64,
    pub scheduled_at: ScheduleAt,
}

#[reducer]
pub fn tick_world_time(ctx: &ReducerContext, _tick: WorldClockTick) {
    let clock = ctx.db.world_clock().id().find(0u8);
    if let Some(existing) = clock {
        let next = (existing.minutes_of_day + 1) % 1440;
        ctx.db.world_clock().id().update(WorldClock {
            minutes_of_day: next,
            last_tick: ctx.timestamp,
            ..existing
        });
    }
    ctx.db.world_clock_tick().insert(WorldClockTick {
        scheduled_id: 0,
        scheduled_at: ScheduleAt::Time(
            ctx.timestamp + std::time::Duration::from_secs(1)
        ),
    });
}

#[reducer]
pub fn change_weather(
    ctx: &ReducerContext,
    zone_id: u64,
    kind: WeatherKind,
    intensity: f32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("not admin".to_string());
    }
    if !(0.0..=1.0).contains(&intensity) {
        return Err("intensity must be 0.0..=1.0".to_string());
    }
    if let Some(existing) = ctx.db.weather_state().zone_id().find(zone_id) {
        ctx.db.weather_state().zone_id().update(WeatherState {
            kind,
            intensity,
            started_at: ctx.timestamp,
            ..existing
        });
    } else {
        ctx.db.weather_state().insert(WeatherState {
            zone_id,
            kind,
            intensity,
            started_at: ctx.timestamp,
        });
    }
    Ok(())
}

#[reducer]
pub fn set_zone_mood(
    ctx: &ReducerContext,
    zone_id: u64,
    mood_preset_id: u32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("not admin".to_string());
    }
    let zone = ctx.db.zone().id().find(zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;
    ctx.db.zone().id().update(crate::Zone { mood_preset_id, ..zone });
    Ok(())
}
