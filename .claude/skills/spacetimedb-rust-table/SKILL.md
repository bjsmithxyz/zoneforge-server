---
name: spacetimedb-rust-table
description: Define a new SpacetimeDB table in the ZoneForge Rust server module with correct syntax and attributes. Use this skill whenever the user wants to add a table, define a schema, store new data, create a new entity type, or model any persistent data on the server. Also trigger when reviewing or fixing existing table definitions — the SpacetimeDB Rust macro system has several sharp edges that LLMs commonly get wrong.
---

## The core rule

Tables use the `#[table(...)]` macro on a `pub struct`. This macro handles serialisation automatically.

**Never add `#[derive(SpacetimeType)]` to a table struct.** It conflicts with the macro and causes a compilation error. `SpacetimeType` is only for custom types that are embedded as *fields* inside table rows.

## Required imports

```rust
use spacetimedb::{table, Table, ReducerContext, Identity, Timestamp};
//                        ^^^^^ required for .insert(), .iter(), .find(), etc.
```

Without the `Table` trait import, table methods won't exist and the compiler gives a confusing "no method named `insert`" error.

## Basic table definition

```rust
#[table(accessor = zone, public)]
pub struct Zone {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub owner: Identity,
    pub name: String,
    pub created_at: Timestamp,
}
```

### Visibility

```rust
#[table(accessor = zone)]           // Private — server only
#[table(accessor = zone, public)]   // Public — clients can subscribe
```

Make a table `public` if Unity clients need to read its rows via subscription. A missing `public` flag means clients will silently receive no data.

### Column attributes

| Attribute | Effect |
|-----------|--------|
| `#[primary_key]` | Unique, indexed, enables `.find()` |
| `#[auto_inc]` | Auto-increment — use with `#[primary_key]` on `u64` |
| `#[unique]` | Unique constraint, auto-indexed, enables `.find()` |
| `#[index(btree)]` | B-Tree index for range queries / `.filter()` |

### Primary key with auto-increment

```rust
#[primary_key]
#[auto_inc]
pub id: u64,
```

When inserting, pass `id: 0` — SpacetimeDB replaces it with the real value. The `insert()` call returns the inserted row with the actual ID:

```rust
let row = ctx.db.zone().insert(Zone { id: 0, name: "...", ... });
let actual_id = row.id;
```

### B-Tree index for filtering

```rust
#[table(accessor = message, public, index(name = by_room, btree(columns = [room_id])))]
pub struct Message {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub room_id: u64,   // indexed for .filter()
    pub text: String,
    pub sent: Timestamp,
}
```

**Index names must be unique across the entire module** — if two tables declare an index with the same name, the build will fail.

## Custom embedded types

Use `#[derive(SpacetimeType)]` **only** for structs or enums that live as fields inside a table row, never on the table struct itself:

```rust
// ✅ Custom type for use as a field
#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum ZoneType {
    Town,
    Dungeon,
    Wilderness,
}

// ✅ Table using custom types — no SpacetimeType here!
#[table(accessor = zone, public)]
pub struct Zone {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub position: Position,
    pub kind: ZoneType,
}
```

## Accessor naming

The `accessor` value becomes the method name used to access the table in reducers: `ctx.db.zone()`. Use `snake_case`, keep it short, and make it singular (one row = one zone).

## Full example

```rust
use spacetimedb::{table, Table, ReducerContext, Identity, Timestamp, SpacetimeType};

#[derive(SpacetimeType, Clone, Debug, PartialEq)]
pub enum TileKind {
    Grass,
    Stone,
    Water,
}

#[table(accessor = tile, public, index(name = tile_by_zone, btree(columns = [zone_id])))]
pub struct Tile {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub zone_id: u64,
    pub x: i32,
    pub y: i32,
    pub kind: TileKind,
    pub placed_by: Identity,
    pub placed_at: Timestamp,
}
```

## Common mistakes

| Wrong | Right |
|-------|-------|
| `#[derive(SpacetimeType)]` on a table | Remove it — the macro handles serialisation |
| `ctx.db.zone` (field) | `ctx.db.zone()` (method call with parentheses) |
| `ctx.db.zone().find(id)` | `ctx.db.zone().id().find(&id)` — must go through the index |
| Forgetting `use spacetimedb::Table` | Add the import — required for all table methods |
| `#[table(accessor = "zone")]` | `#[table(accessor = zone)]` — no string literals |
| Duplicate index name across tables | Each index name must be unique in the module |
| Missing `public` when clients need the data | Add `public` — clients can't subscribe to private tables |
