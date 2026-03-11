# ZoneForge Server

SpacetimeDB Rust server module for ZoneForge — a 3D multiplayer RPG world builder. Defines the database schema, game state tables, and server-authoritative reducers.

## Stack

- **Language**: Rust (compiled to WASM)
- **Platform**: SpacetimeDB 2.x
- **Build target**: `wasm32-unknown-unknown`

## Project Structure

```
server/
├── spacetime.json         # SpacetimeDB project config (module-path → ./spacetimedb)
└── spacetimedb/
    ├── src/
    │   └── lib.rs         # All tables and reducers
    └── Cargo.toml         # spacetimedb = "2.0", crate-type = ["cdylib"]
```

## Prerequisites

- Rust toolchain with `wasm32-unknown-unknown` target:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```

- SpacetimeDB CLI:

```bash
curl -fsSL https://install.spacetimedb.com | bash
```

## Local Development

```bash
# Start the local SpacetimeDB server
spacetime start

# Build the module (run from server/)
spacetime build

# Publish to local server
spacetime publish --server local zoneforge-server

# Republish after a breaking schema change (wipes all data)
spacetime publish --server local zoneforge-server --delete-data

# View server logs
spacetime logs zoneforge-server
```

## Generating Client Bindings

Run from the Unity client or editor project directory:

```bash
spacetime generate --lang csharp \
  --out-dir Assets/Scripts/autogen \
  --bin-path ../server/spacetimedb/target/wasm32-unknown-unknown/release/zoneforge_server.wasm
```

## Tables

| Table | Description |
|-------|-------------|
| `player` | Connected players with position, health, and zone |
| `zone` | Zone definitions with grid dimensions |
| `entity_instance` | Entities placed in zones (props, NPCs, enemies) |

## Reducers

| Reducer | Description |
|---------|-------------|
| `create_player` | Register a new player on connect |
| `move_player` | Update player position (server-authoritative) |
| `create_zone` | Create a new named zone |
| `spawn_entity` | Place an entity in a zone |

## Related

- [zoneforge-client](https://github.com/bjsmithxyz/zoneforge-client) — Unity game client
- [zoneforge-editor](https://github.com/bjsmithxyz/zoneforge-editor) — Standalone world editor
- [zoneforge](https://github.com/bjsmithxyz/zoneforge) — Umbrella repo and documentation
