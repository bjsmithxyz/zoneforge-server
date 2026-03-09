# ZoneForge Server

SpacetimeDB Rust server module for ZoneForge — a tile-based multiplayer world builder. Defines the database schema, game state tables, and server-authoritative reducers.

## Stack

- **Language**: Rust
- **Platform**: SpacetimeDB 1.x (resolved: 1.12.0)
- **Build target**: `wasm32-unknown-unknown`

## Project Structure

```
spacetimedb/
├── src/
│   └── lib.rs      # Tables and reducers
└── Cargo.toml
spacetime.json       # SpacetimeDB project config
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

# Build the module
spacetime build --project-path spacetimedb

# Publish to local server
spacetime publish zoneforge-server --project-path spacetimedb

# Republish after a breaking schema change (clears all data)
spacetime publish zoneforge-server --project-path spacetimedb --clear-database -y

# View server logs
spacetime logs zoneforge-server
```

## Generating Client Bindings

Run from the Unity client project root:

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

## Reducers

| Reducer | Description |
|---------|-------------|
| `create_player` | Register a new player on connect |
| `move_player` | Update player position (server-authoritative) |
| `create_zone` | Create a new named zone |

## Related

- [zoneforge-client](https://github.com/bjsmithxyz/zoneforge-client) — Unity game client
- [zoneforge](https://github.com/bjsmithxyz/zoneforge) — Umbrella repo and documentation
