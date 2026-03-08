use spacetimedb::{table, reducer, ReducerContext, Identity, Table};

// Define a simple Player table.
// Note: #[table(...)] is the table attribute — do NOT also add #[derive(SpacetimeType)].
// SpacetimeType is only for custom embedded types used as fields inside table rows.
#[table(name = player, public)]
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
#[table(name = zone, public)]
pub struct Zone {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub name: String,
    pub grid_width: u32,
    pub grid_height: u32,
}

// Reducer to create a new player
#[reducer]
pub fn create_player(ctx: &ReducerContext, name: String) {
    let player = Player {
        id: 0, // auto_inc will assign
        identity: ctx.sender,
        name,
        zone_id: 1, // Default starting zone
        position_x: 0.0,
        position_y: 0.0,
        health: 100,
        max_health: 100,
    };

    ctx.db.player().insert(player);
    log::info!("Player created: {}", ctx.sender);
}

// Reducer to move a player
#[reducer]
pub fn move_player(ctx: &ReducerContext, new_x: f32, new_y: f32) {
    let player_identity = ctx.sender;

    // .identity() works here because the field is marked #[unique] above.
    if let Some(player) = ctx.db.player().identity().find(&player_identity) {
        ctx.db.player().id().update(Player {
            position_x: new_x,
            position_y: new_y,
            ..player
        });
        log::info!("Player moved to ({}, {})", new_x, new_y);
    }
}

// Reducer to create a zone
#[reducer]
pub fn create_zone(ctx: &ReducerContext, name: String, width: u32, height: u32) {
    let zone = Zone {
        id: 0, // auto_inc
        name: name.clone(),
        grid_width: width,
        grid_height: height,
    };

    ctx.db.zone().insert(zone);
    log::info!("Zone created: {}", name);
}