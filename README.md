# factorio-rcon

Async RCON client for Factorio with proper multi-packet response handling.

Factorio can return large responses (>64KB) that get fragmented across multiple TCP segments. This crate correctly handles that by reading the length-prefixed packet header first, then using `read_exact()` to buffer the complete packet body before parsing.

## Usage

```rust
use factorio_rcon::RconClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> factorio_rcon::Result<()> {
    // Connect and authenticate
    let mut client = RconClient::connect("127.0.0.1:27015", "password").await?;

    // Execute basic commands
    let version = client.execute("/version").await?;
    println!("Server version: {}", version);

    // Execute Lua code
    let tick = client.execute("/c rcon.print(game.tick)").await?;
    println!("Current tick: {}", tick);

    // Query with serpent serialization (for structured data)
    let inventory = client.execute(
        "/c rcon.print(serpent.line(game.connected_players[1].get_main_inventory()))"
    ).await?;

    // Custom timeout for slow queries
    let result = client.execute_with_timeout(
        "/c rcon.print(serpent.line(game.surfaces))",
        Duration::from_secs(10)
    ).await?;

    // Configure default timeout
    client.set_timeout(Duration::from_secs(15));

    Ok(())
}
```

## Prerequisites

Factorio must run as a multiplayer host for RCON to work:

1. Launch Factorio
2. Multiplayer → Host New Game
3. RCON is automatically enabled (check `config.ini` for port and password)

Default RCON configuration:
- Port: 27015
- Password: Check `config/config.ini` in your Factorio installation

**Note:** Using `/c` commands (Lua execution) disables achievements for that save file.

## Features

- ✅ Async/await with Tokio
- ✅ Correct multi-packet fragmentation handling
- ✅ Strong error types with thiserror
- ✅ Configurable timeouts
- ✅ Lua execution support with serpent serialization
- ✅ Tracing support for debugging

## Error Handling

The crate provides strongly-typed errors for different failure scenarios:

```rust
use factorio_rcon::{RconClient, RconError};

match RconClient::connect("127.0.0.1:27015", "wrong_password").await {
    Ok(client) => println!("Connected!"),
    Err(RconError::ConnectionFailed(e)) => println!("Can't reach server: {}", e),
    Err(RconError::AuthFailed) => println!("Wrong password"),
    Err(RconError::Timeout(ms)) => println!("Timed out after {}ms", ms),
    Err(RconError::ConnectionLost(e)) => println!("Disconnected: {}", e),
    Err(e) => println!("Other error: {}", e),
}
```

## Testing

Unit tests can be run without a Factorio server:

```bash
cargo test
```

Integration tests require a running Factorio server and are ignored by default:

```bash
# Start Factorio as multiplayer host first
cargo test -- --ignored
```

## RCON Protocol Details

RCON uses a binary protocol over TCP:

```
[4 bytes: length]      # payload + 10 (excludes this length field)
[4 bytes: request ID]  # incremented per command
[4 bytes: type]        # 3=auth, 2=command, 0=response
[N bytes: payload]     # UTF-8 string
[2 bytes: null term]   # 0x00 0x00
```

All integers are little-endian.

## License

MIT OR Apache-2.0
