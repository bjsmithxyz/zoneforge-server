---
name: spacetimedb-rust-reducer
description: Implement a SpacetimeDB reducer in the ZoneForge Rust server module with correct patterns. Use this skill whenever the user wants to add a server-side action, handle a client request, mutate table data, implement a game mechanic on the server, or handle player events. Also trigger when reviewing or fixing existing reducer code — the SpacetimeDB Rust API has subtle rules that LLMs commonly violate (wrong return type, mutable context, returning data, non-determinism).
---

## What a reducer is

A reducer is a transactional function that runs on the server in response to a client call (or a scheduled trigger). It can read and write tables, but:
- It cannot return data to the caller — clients read data via subscriptions, not return values
- It must be deterministic — no filesystem, network, external timers, or the `rand` crate
- It runs to completion or fails atomically

## Required imports

```rust
use spacetimedb::{reducer, ReducerContext, Table};
//                                          ^^^^^ required for table methods
```

## Basic reducer

```rust
#[reducer]
pub fn place_tile(ctx: &ReducerContext, zone_id: u64, x: i32, y: i32, kind: TileKind) -> Result<(), String> {
    if !zone_exists(ctx, zone_id) {
        return Err(format!("Zone {} not found", zone_id));
    }

    ctx.db.tile().insert(Tile {
        id: 0,  // auto-inc placeholder — SpacetimeDB fills this in
        zone_id,
        x,
        y,
        kind,
        placed_by: ctx.sender,
        placed_at: ctx.timestamp,
    });

    Ok(())
}
```

### Signature rules

| Rule | Why |
|------|-----|
| `ctx: &ReducerContext` — immutable borrow | SpacetimeDB enforces this; `&mut` causes a compile error |
| Return `Result<(), String>` for fallible paths | Returning `Err(...)` rolls back the transaction and surfaces the error to the client without crashing the WASM instance |
| Return `()` (nothing) only if the reducer can never fail | Fine for simple lifecycle hooks |
| Never return data | Clients read state through subscriptions |

## ReducerContext fields

```rust
ctx.sender       // Identity of the calling client — always authenticated, never spoofable
ctx.timestamp    // Current server timestamp — use instead of SystemTime::now()
ctx.db           // Database accessor — ctx.db.table_name()
ctx.rng          // Deterministic RNG — use instead of the rand crate
```

**Never trust an identity passed as a parameter** — always use `ctx.sender` for the caller's identity.

## Update pattern

SpacetimeDB has no partial update. To change fields, find the row, spread it, and override only what changes:

```rust
#[reducer]
pub fn rename_zone(ctx: &ReducerContext, zone_id: u64, new_name: String) -> Result<(), String> {
    let zone = ctx.db.zone().id().find(&zone_id)
        .ok_or_else(|| format!("Zone {} not found", zone_id))?;

    // Spread preserves all other fields
    ctx.db.zone().id().update(Zone { name: new_name, ..zone });

    Ok(())
}
```

Never reconstruct the full row with `..Default::default()` — that zeros out all other fields.

## Delete pattern

```rust
#[reducer]
pub fn remove_tile(ctx: &ReducerContext, tile_id: u64) -> Result<(), String> {
    ctx.db.tile().id().delete(&tile_id);
    Ok(())
}
```

## Borrow-after-move pitfall

Move values into structs before checking them, or clone first:

```rust
// ❌ WRONG — `kind` is moved, then checked
ctx.db.tile().insert(Tile { kind, ... });
if kind == TileKind::Water { ... }  // ERROR: value moved

// ✅ CORRECT — check before move, or clone
let is_water = kind == TileKind::Water;
ctx.db.tile().insert(Tile { kind, ... });
if is_water { ... }
```

## Lifecycle hooks

```rust
#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    // Called once when the module is first published
    // Good place to seed initial data
}

#[reducer(client_connected)]
pub fn client_connected(ctx: &ReducerContext) {
    // ctx.sender is the connecting identity
    if let Some(player) = ctx.db.player().identity().find(ctx.sender) {
        ctx.db.player().identity().update(Player { online: true, ..player });
    } else {
        ctx.db.player().insert(Player {
            identity: ctx.sender,
            name: None,
            online: true,
        });
    }
}

#[reducer(client_disconnected)]
pub fn client_disconnected(ctx: &ReducerContext) {
    if let Some(player) = ctx.db.player().identity().find(ctx.sender) {
        ctx.db.player().identity().update(Player { online: false, ..player });
    }
}
```

## Scheduled reducers

```rust
use spacetimedb::{table, ScheduleAt};

#[table(accessor = tick_timer, scheduled(game_tick))]
pub struct TickTimer {
    #[primary_key]
    #[auto_inc]
    scheduled_id: u64,
    scheduled_at: ScheduleAt,
}

#[reducer]
pub fn game_tick(ctx: &ReducerContext, _timer: TickTimer) -> Result<(), String> {
    // Runs on schedule; timer row is deleted after this returns
    Ok(())
}
```

Use `ScheduleAt::Time(timestamp)`, not `ScheduleAt::At(...)` — the `At` variant doesn't exist.

## Logging

```rust
use spacetimedb::log;

log::info!("Player {:?} placed tile at ({}, {})", ctx.sender, x, y);
log::warn!("Zone {} is approaching capacity", zone_id);
log::error!("Unexpected state in reducer");
```

View logs with `spacetime logs zoneforge-server`.

## Determinism rules

| Forbidden | Use instead |
|-----------|-------------|
| `rand::thread_rng()` | `ctx.rng` |
| `std::time::SystemTime::now()` | `ctx.timestamp` |
| `std::fs::*` | Not possible in reducers — use procedures |
| HTTP / network calls | Not possible in reducers — use procedures |

## Common mistakes

| Wrong | Right |
|-------|-------|
| `ctx: &mut ReducerContext` | `ctx: &ReducerContext` |
| Returning data: `-> Option<Player>` | Return `Result<(), String>` or `()` |
| `use spacetimedb_sdk::*` | `use spacetimedb::*` — sdk crate is for clients |
| `ctx.db.player` (field) | `ctx.db.player()` (method) |
| `ctx.db.player().find(id)` | `ctx.db.player().id().find(&id)` |
| `rand::random::<u32>()` | `ctx.rng.next_u32()` |
| `SystemTime::now()` | `ctx.timestamp` |
| `panic!("error")` | `return Err("error".to_string())` |
| Trust identity param | Use `ctx.sender` |
| `..Default::default()` in update | `..existing_row` — spread the found row |
