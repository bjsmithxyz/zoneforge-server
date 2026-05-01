use spacetimedb::{table, reducer, ReducerContext, SpacetimeType, Table};

use crate::is_admin;
use crate::player as _;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum ItemType {
    Weapon,
    Armor,
    Accessory,
    Consumable,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

#[table(accessor = item_def, public)]
pub struct ItemDefinition {
    #[primary_key]
    #[auto_inc]
    pub id:           u64,
    pub name:         String,
    pub description:  String,
    pub item_type:    ItemType,
    pub rarity:       Rarity,
    pub icon_name:    String,
    pub damage_bonus: i32,
    pub armor_bonus:  i32,
    pub healing:      i32,
    pub value:        u32,
}

#[table(accessor = inventory, public)]
pub struct Inventory {
    #[primary_key]
    #[auto_inc]
    pub id:          u64,
    #[index(btree)]
    pub player_id:   u64,
    pub item_def_id: u64,
    pub quantity:    u32,
}

#[table(accessor = equipment, public)]
pub struct Equipment {
    #[primary_key]
    pub player_id:    u64,
    pub weapon_id:    Option<u64>,
    pub armor_id:     Option<u64>,
    pub accessory_id: Option<u64>,
}

pub(crate) fn add_to_inventory(ctx: &ReducerContext, player_id: u64, item_def_id: u64, quantity: u32) {
    if let Some(existing) = ctx.db.inventory()
        .player_id()
        .filter(&player_id)
        .find(|row| row.item_def_id == item_def_id)
    {
        let new_qty = existing.quantity.saturating_add(quantity);
        ctx.db.inventory().id().update(Inventory {
            quantity: new_qty,
            ..existing
        });
    } else {
        ctx.db.inventory().insert(Inventory {
            id: 0,
            player_id,
            item_def_id,
            quantity,
        });
    }
}

pub(crate) fn remove_from_inventory(ctx: &ReducerContext, player_id: u64, item_def_id: u64, quantity: u32) -> bool {
    if quantity == 0 {
        return false;
    }
    if let Some(row) = ctx.db.inventory()
        .player_id()
        .filter(&player_id)
        .find(|r| r.item_def_id == item_def_id)
    {
        if row.quantity < quantity {
            return false;
        }
        if row.quantity == quantity {
            ctx.db.inventory().id().delete(&row.id);
        } else {
            ctx.db.inventory().id().update(Inventory {
                quantity: row.quantity - quantity,
                ..row
            });
        }
        true
    } else {
        false
    }
}

#[reducer]
pub fn create_item_def(
    ctx: &ReducerContext,
    name: String,
    description: String,
    item_type: ItemType,
    rarity: Rarity,
    icon_name: String,
    damage_bonus: i32,
    armor_bonus: i32,
    healing: i32,
    value: u32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    if name.is_empty() || name.len() > 64 || name.contains('\0') {
        return Err("name must be 1–64 characters, no null bytes".to_string());
    }
    ctx.db.item_def().insert(ItemDefinition {
        id: 0,
        name,
        description,
        item_type,
        rarity,
        icon_name,
        damage_bonus,
        armor_bonus,
        healing,
        value,
    });
    Ok(())
}

#[reducer]
pub fn delete_item_def(ctx: &ReducerContext, item_def_id: u64) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.item_def().id().find(&item_def_id)
        .ok_or_else(|| format!("ItemDefinition {} not found", item_def_id))?;
    ctx.db.item_def().id().delete(&item_def_id);
    Ok(())
}

#[reducer]
pub fn give_item(
    ctx: &ReducerContext,
    player_id: u64,
    item_def_id: u64,
    quantity: u32,
) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("Not authorized: admin only".to_string());
    }
    ctx.db.player().id().find(&player_id)
        .ok_or_else(|| "Player not found".to_string())?;
    ctx.db.item_def().id().find(&item_def_id)
        .ok_or_else(|| "ItemDefinition not found".to_string())?;
    if quantity == 0 {
        return Err("quantity must be > 0".to_string());
    }
    add_to_inventory(ctx, player_id, item_def_id, quantity);
    Ok(())
}

#[reducer]
pub fn equip_item(ctx: &ReducerContext, item_def_id: u64) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or_else(|| "Player not found".to_string())?;
    if player.is_dead {
        return Err("Cannot equip items while dead".to_string());
    }
    let def = ctx.db.item_def().id().find(&item_def_id)
        .ok_or_else(|| "ItemDefinition not found".to_string())?;

    if def.item_type == ItemType::Consumable {
        return Err("Consumables cannot be equipped".to_string());
    }

    if !remove_from_inventory(ctx, player.id, item_def_id, 1) {
        return Err("Item not in inventory".to_string());
    }

    let equipment_exists = ctx.db.equipment().player_id().find(&player.id).is_some();

    let eq = ctx.db.equipment().player_id().find(&player.id)
        .unwrap_or(Equipment {
            player_id:    player.id,
            weapon_id:    None,
            armor_id:     None,
            accessory_id: None,
        });

    let (old_id, new_eq) = match def.item_type {
        ItemType::Weapon    => (eq.weapon_id,    Equipment { weapon_id:    Some(item_def_id), ..eq }),
        ItemType::Armor     => (eq.armor_id,     Equipment { armor_id:     Some(item_def_id), ..eq }),
        ItemType::Accessory => (eq.accessory_id, Equipment { accessory_id: Some(item_def_id), ..eq }),
        ItemType::Consumable => unreachable!("guarded above"),
    };

    if let Some(prev_id) = old_id {
        add_to_inventory(ctx, player.id, prev_id, 1);
    }

    if equipment_exists {
        ctx.db.equipment().player_id().update(new_eq);
    } else {
        ctx.db.equipment().insert(new_eq);
    }
    Ok(())
}

#[reducer]
pub fn unequip_item(ctx: &ReducerContext, slot: String) -> Result<(), String> {
    let player = ctx.db.player().identity().find(ctx.sender())
        .ok_or_else(|| "Player not found".to_string())?;
    if player.is_dead {
        return Err("Cannot unequip items while dead".to_string());
    }
    let eq = ctx.db.equipment().player_id().find(&player.id)
        .ok_or_else(|| "No equipment row found".to_string())?;

    let (item_id, new_eq) = match slot.as_str() {
        "weapon" => (
            eq.weapon_id.ok_or_else(|| "Weapon slot is empty".to_string())?,
            Equipment { weapon_id: None, ..eq },
        ),
        "armor" => (
            eq.armor_id.ok_or_else(|| "Armor slot is empty".to_string())?,
            Equipment { armor_id: None, ..eq },
        ),
        "accessory" => (
            eq.accessory_id.ok_or_else(|| "Accessory slot is empty".to_string())?,
            Equipment { accessory_id: None, ..eq },
        ),
        _ => return Err(format!("Unknown slot '{}'. Use weapon/armor/accessory", slot)),
    };

    add_to_inventory(ctx, player.id, item_id, 1);
    ctx.db.equipment().player_id().update(new_eq);
    Ok(())
}
