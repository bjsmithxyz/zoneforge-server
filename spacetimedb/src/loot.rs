use spacetimedb::{table, reducer, ReducerContext, Table};

use crate::is_admin;
use crate::inventory::add_to_inventory;
use crate::Enemy;
use crate::player as _;
use crate::enemy::enemy_def as _;
use crate::inventory::item_def as _;

#[table(accessor = loot_table, public)]
pub struct LootTable {
    #[primary_key]
    #[auto_inc]
    pub id:           u64,
    #[index(btree)]
    pub enemy_def_id: u64,
    pub item_def_id:  u64,
    pub drop_chance:  u32,
    pub min_quantity: u32,
    pub max_quantity: u32,
}

#[table(accessor = item_drop, public)]
pub struct ItemDrop {
    #[primary_key]
    #[auto_inc]
    pub id:          u64,
    #[index(btree)]
    pub zone_id:     u64,
    pub item_def_id: u64,
    pub quantity:    u32,
    pub pos_x:       f32,
    pub pos_y:       f32,
}

pub(crate) fn spawn_loot_drops(ctx: &ReducerContext, enemy: &Enemy) {
    for entry in ctx.db.loot_table().enemy_def_id().filter(&enemy.enemy_def_id) {
        let seed = enemy.id
            .wrapping_mul(0x9e3779b97f4a7c15)
            .wrapping_add(
                ctx.timestamp
                    .to_duration_since_unix_epoch()
                    .unwrap_or_default()
                    .as_micros() as u64,
            )
            .wrapping_add(entry.id);
        if entry.drop_chance == 0 {
            continue;
        }
        let roll = (seed % 100 + 1) as u32;
        if roll > entry.drop_chance {
            continue;
        }
        let range = entry.max_quantity - entry.min_quantity + 1;
        let qty = entry.min_quantity + (seed.wrapping_mul(0x6c62272e07bb0142) % range as u64) as u32;
        let offset_x = ((seed & 0xff) as f32 / 255.0 - 0.5) * 1.5;
        let offset_y = (((seed >> 8) & 0xff) as f32 / 255.0 - 0.5) * 1.5;
        ctx.db.item_drop().insert(ItemDrop {
            id:          0,
            zone_id:     enemy.zone_id,
            item_def_id: entry.item_def_id,
            quantity:    qty,
            pos_x:       enemy.position_x + offset_x,
            pos_y:       enemy.position_y + offset_y,
        });
    }
}

#[reducer]
pub fn create_loot_table(
    ctx: &ReducerContext,
    enemy_def_id: u64,
    item_def_id:  u64,
    drop_chance:  u32,
    min_quantity: u32,
    max_quantity: u32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    if drop_chance > 100 {
        return Err("drop_chance must be 0–100".to_string());
    }
    if min_quantity == 0 || max_quantity < min_quantity {
        return Err("min_quantity must be >= 1 and max_quantity >= min_quantity".to_string());
    }
    ctx.db.enemy_def().id().find(&enemy_def_id)
        .ok_or_else(|| "EnemyDefinition not found".to_string())?;
    ctx.db.item_def().id().find(&item_def_id)
        .ok_or_else(|| "ItemDefinition not found".to_string())?;
    ctx.db.loot_table().insert(LootTable {
        id: 0,
        enemy_def_id,
        item_def_id,
        drop_chance,
        min_quantity,
        max_quantity,
    });
    Ok(())
}

#[reducer]
pub fn delete_loot_table(ctx: &ReducerContext, loot_table_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.loot_table().id().find(&loot_table_id)
        .ok_or_else(|| format!("LootTable {} not found", loot_table_id))?;
    ctx.db.loot_table().id().delete(&loot_table_id);
    Ok(())
}

#[reducer]
pub fn pickup_item(ctx: &ReducerContext, item_drop_id: u64) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or_else(|| "Player not found".to_string())?;
    if player.is_dead {
        return Err("Cannot pick up items while dead".to_string());
    }
    let drop = ctx.db.item_drop().id().find(&item_drop_id)
        .ok_or_else(|| "ItemDrop not found".to_string())?;
    if drop.zone_id != player.zone_id {
        return Err("ItemDrop is not in your zone".to_string());
    }
    let dx = player.position_x - drop.pos_x;
    let dz = player.position_y - drop.pos_y;
    if dx * dx + dz * dz > 4.0 {
        return Err("Too far from item drop".to_string());
    }
    ctx.db.item_drop().id().delete(&item_drop_id);
    add_to_inventory(ctx, player.id, drop.item_def_id, drop.quantity);
    spacetimedb::log::info!(
        "pickup_item: player={} picked up item_def={} x{}",
        player.id, drop.item_def_id, drop.quantity
    );
    Ok(())
}
