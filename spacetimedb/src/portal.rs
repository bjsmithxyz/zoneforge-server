use spacetimedb::{table, reducer, ReducerContext, Table};

use crate::{dist_sq, Player};
use crate::{player as _, zone as _};

#[table(accessor = portal, public)]
pub struct Portal {
    #[primary_key]
    #[auto_inc]
    pub id:             u64,
    #[index(btree)]
    pub source_zone_id: u64,
    #[index(btree)]
    pub dest_zone_id:   u64,
    pub source_x:       f32,
    pub source_y:       f32,
    pub dest_spawn_x:   f32,
    pub dest_spawn_y:   f32,
    pub bidirectional:  bool,
    pub label:          String,
}

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
    spacetimedb::log::info!("create_portal: {} -> {} at ({},{})", source_zone_id, dest_zone_id, source_x, source_y);
    Ok(())
}

#[reducer]
pub fn delete_portal(ctx: &ReducerContext, portal_id: u64) -> Result<(), String> {
    ctx.db.portal().id().find(&portal_id)
        .ok_or_else(|| format!("Portal {} not found", portal_id))?;
    ctx.db.portal().id().delete(&portal_id);
    spacetimedb::log::info!("delete_portal: {}", portal_id);
    Ok(())
}

#[reducer]
pub fn enter_zone(ctx: &ReducerContext) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or("Player not found")?;
    if player.is_dead {
        return Err("Dead players cannot use portals".to_string());
    }

    let zone_id = player.zone_id;
    let px = player.position_x;
    let py = player.position_y;

    for portal in ctx.db.portal().source_zone_id().filter(&zone_id) {
        if dist_sq(px, py, portal.source_x, portal.source_y) < 4.0 {
            let dest_zone_id = portal.dest_zone_id;
            ctx.db.player().id().update(Player {
                zone_id:    portal.dest_zone_id,
                position_x: portal.dest_spawn_x,
                position_y: portal.dest_spawn_y,
                ..player
            });
            spacetimedb::log::info!("enter_zone: {:?} -> zone {}", ctx.sender(), dest_zone_id);
            return Ok(());
        }
    }

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
            spacetimedb::log::info!("enter_zone: {:?} (reverse) -> zone {}", ctx.sender(), src_zone_id);
            return Ok(());
        }
    }

    Err("No portal within range".to_string())
}
