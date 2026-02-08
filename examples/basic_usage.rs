use factorio_rcon::RconClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> factorio_rcon::Result<()> {
    // Connect to Factorio RCON server
    // Make sure Factorio is running as multiplayer host
    let mut client = RconClient::connect("127.0.0.1:27015", "password").await?;

    println!("✅ Connected to Factorio RCON server\n");

    // Execute basic commands
    println!("=== Basic Commands ===");
    let version = client.execute("/version").await?;
    println!("Server version: {}", version);

    // Execute Lua code
    println!("\n=== Lua Execution ===");
    let tick = client.execute("/c rcon.print(game.tick)").await?;
    println!("Current game tick: {}", tick);

    let seed = client.execute("/c rcon.print(game.surfaces[1].map_gen_settings.seed)").await?;
    println!("Map seed: {}", seed);

    // Query with serpent serialization for structured data
    println!("\n=== Serpent Serialization ===");
    let player_count = client
        .execute("/c rcon.print(serpent.line(#game.connected_players))")
        .await?;
    println!("Connected players: {}", player_count);

    if player_count.trim() != "0" {
        let player_name = client
            .execute("/c rcon.print(serpent.line(game.connected_players[1].name))")
            .await?;
        println!("First player: {}", player_name);
    }

    // Custom timeout for potentially slow queries
    println!("\n=== Custom Timeout ===");
    let surfaces = client
        .execute_with_timeout(
            "/c rcon.print(serpent.line(game.surfaces))",
            Duration::from_secs(10),
        )
        .await?;
    println!("Surfaces (first 100 chars): {}...", &surfaces[..surfaces.len().min(100)]);

    // Configure default timeout
    client.set_timeout(Duration::from_secs(15));
    println!("\n✅ Default timeout set to 15 seconds");

    println!("\n=== Done ===");

    Ok(())
}
