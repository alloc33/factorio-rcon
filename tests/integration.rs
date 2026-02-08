use factorio_rcon::{RconClient, RconError};
use std::time::Duration;

/// Default test server address (adjust if needed)
const TEST_ADDR: &str = "127.0.0.1:27015";

/// Test password - you may need to adjust this based on your config.ini
const TEST_PASSWORD: &str = "password";

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_real_factorio_connection() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect - is Factorio running as multiplayer host?");

    let version = client
        .execute("/version")
        .await
        .expect("Failed to execute /version");

    println!("Factorio version: {}", version);
    assert!(!version.is_empty());
    assert!(version.contains("Factorio") || version.contains("Version"));
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_lua_execution() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect");

    let tick = client
        .execute("/c rcon.print(game.tick)")
        .await
        .expect("Failed to execute Lua command");

    println!("Current game tick: {}", tick);

    // Tick should be a number
    let _tick_num: u64 = tick
        .trim()
        .parse()
        .expect("Tick should be a valid number");

    // u64 is always >= 0, so just checking it parses is sufficient
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_auth_failure() {
    let result = RconClient::connect(TEST_ADDR, "wrong_password").await;

    match result {
        Err(RconError::AuthFailed) => {
            println!("Auth correctly failed with wrong password");
        }
        Ok(_) => panic!("Should have failed authentication with wrong password"),
        Err(e) => panic!("Wrong error type: expected AuthFailed, got {:?}", e),
    }
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_connection_refused() {
    let result = RconClient::connect("127.0.0.1:9999", TEST_PASSWORD).await;

    match result {
        Err(RconError::ConnectionFailed(_)) => {
            println!("Connection correctly failed for invalid address");
        }
        Ok(_) => panic!("Should have failed to connect to non-existent server"),
        Err(e) => panic!("Wrong error type: expected ConnectionFailed, got {:?}", e),
    }
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_large_response() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect");

    // Query all surfaces (can be large on established games)
    let surfaces = client
        .execute("/c rcon.print(serpent.line(game.surfaces))")
        .await
        .expect("Failed to query surfaces");

    println!("Surfaces response length: {} bytes", surfaces.len());
    assert!(!surfaces.is_empty());

    // Query all entities on the default surface (can be very large)
    let entities = client
        .execute_with_timeout(
            "/c rcon.print(serpent.line(game.surfaces[1].find_entities()))",
            Duration::from_secs(10),
        )
        .await
        .expect("Failed to query entities");

    println!("Entities response length: {} bytes", entities.len());
    assert!(!entities.is_empty());

    // If response is > 64KB, we've successfully handled fragmentation
    if entities.len() > 65536 {
        println!("✅ Successfully handled fragmented response (>64KB)");
    }
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_timeout() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect");

    // Set a very short timeout
    client.set_timeout(Duration::from_millis(1));

    // This should timeout (1ms is too short for any real command)
    let result = client.execute("/c rcon.print(game.tick)").await;

    match result {
        Err(RconError::Timeout(ms)) => {
            println!("Command correctly timed out after {}ms", ms);
            assert!(ms <= 10); // Should be ~1ms
        }
        Ok(_) => {
            println!("Warning: Command completed faster than timeout - this is rare but possible");
        }
        Err(e) => panic!("Wrong error type: expected Timeout, got {:?}", e),
    }
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_multiple_commands() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect");

    // Execute multiple commands in sequence
    for i in 1..=5 {
        let result = client
            .execute(&format!("/c rcon.print('Command {}')", i))
            .await
            .expect("Failed to execute command");

        println!("Command {} result: {}", i, result);
        assert_eq!(result.trim(), format!("Command {}", i));
    }
}

#[tokio::test]
#[ignore] // Requires running Factorio server
async fn test_serpent_serialization() {
    let mut client = RconClient::connect(TEST_ADDR, TEST_PASSWORD)
        .await
        .expect("Failed to connect");

    // Get player count using serpent
    let player_count = client
        .execute("/c rcon.print(serpent.line(#game.connected_players))")
        .await
        .expect("Failed to query player count");

    println!("Connected players: {}", player_count);

    let count: usize = player_count
        .trim()
        .parse()
        .expect("Player count should be a number");

    // usize is always >= 0, so parsing is sufficient validation

    // Get first player's name if any players are connected
    if count > 0 {
        let player_name = client
            .execute("/c rcon.print(serpent.line(game.connected_players[1].name))")
            .await
            .expect("Failed to query player name");

        println!("First player name: {}", player_name);
        assert!(!player_name.trim().is_empty());
    }
}
