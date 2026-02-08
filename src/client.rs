use crate::error::{RconError, Result};
use crate::protocol::{read_packet, write_packet, Packet};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tracing::{debug, info};

/// Default timeout for connect and command operations
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Async RCON client for Factorio
#[derive(Debug)]
pub struct RconClient {
    stream: TcpStream,
    next_id: i32,
    timeout_duration: Duration,
}

impl RconClient {
    /// Connect to an RCON server and authenticate
    ///
    /// Uses the default 5-second timeout for both TCP connection and auth.
    ///
    /// # Arguments
    /// * `addr` - Server address (e.g., "127.0.0.1:27015")
    /// * `password` - RCON password
    ///
    /// # Example
    /// ```no_run
    /// # async fn example() -> factorio_rcon::Result<()> {
    /// use factorio_rcon::RconClient;
    ///
    /// let mut client = RconClient::connect("127.0.0.1:27015", "password").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(addr: impl AsRef<str>, password: &str) -> Result<Self> {
        let addr = addr.as_ref();
        info!("Connecting to RCON server at {}", addr);

        let stream = timeout(DEFAULT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| RconError::Timeout(DEFAULT_TIMEOUT.as_millis() as u64))?
            .map_err(RconError::ConnectionFailed)?;

        let mut client = Self {
            stream,
            next_id: 1,
            timeout_duration: DEFAULT_TIMEOUT,
        };

        client.authenticate(password).await?;

        info!("Successfully connected and authenticated to {}", addr);
        Ok(client)
    }

    /// Execute an RCON command and return the response
    ///
    /// # Arguments
    /// * `command` - Command to execute (e.g., "/version" or "/c game.tick")
    ///
    /// # Example
    /// ```no_run
    /// # async fn example() -> factorio_rcon::Result<()> {
    /// # use factorio_rcon::RconClient;
    /// # let mut client = RconClient::connect("127.0.0.1:27015", "password").await?;
    /// let version = client.execute("/version").await?;
    /// println!("Server version: {}", version);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute(&mut self, command: &str) -> Result<String> {
        self.execute_with_timeout(command, self.timeout_duration)
            .await
    }

    /// Execute a command with a custom timeout
    ///
    /// The timeout covers the entire round-trip (send + receive).
    ///
    /// # Arguments
    /// * `command` - Command to execute
    /// * `timeout_duration` - Maximum time to wait for the complete operation
    ///
    /// # Example
    /// ```no_run
    /// # async fn example() -> factorio_rcon::Result<()> {
    /// # use factorio_rcon::RconClient;
    /// # use std::time::Duration;
    /// # let mut client = RconClient::connect("127.0.0.1:27015", "password").await?;
    /// let result = client.execute_with_timeout(
    ///     "/c rcon.print(serpent.line(game.surfaces))",
    ///     Duration::from_secs(10)
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute_with_timeout(
        &mut self,
        command: &str,
        timeout_duration: Duration,
    ) -> Result<String> {
        let id = self.next_request_id();
        debug!(id, command, "Executing command");

        let result = timeout(timeout_duration, async {
            let packet = Packet::command(id, command);
            self.send_packet(&packet).await?;
            self.receive_packet().await
        })
        .await
        .map_err(|_| RconError::Timeout(timeout_duration.as_millis() as u64))??;

        if result.id != id {
            return Err(RconError::ProtocolError(format!(
                "Response ID mismatch: expected {}, got {}",
                id, result.id
            )));
        }

        debug!(
            id,
            response_len = result.payload.len(),
            "Command executed successfully"
        );

        Ok(result.payload)
    }

    /// Configure the default timeout for operations
    ///
    /// # Arguments
    /// * `duration` - New timeout duration
    pub fn set_timeout(&mut self, duration: Duration) {
        self.timeout_duration = duration;
        debug!(?duration, "Timeout updated");
    }

    /// Authenticate with the RCON server
    async fn authenticate(&mut self, password: &str) -> Result<()> {
        debug!("Authenticating");

        let id = self.next_request_id();
        let packet = Packet::auth(id, password);

        let response = timeout(self.timeout_duration, async {
            self.send_packet(&packet).await?;
            self.receive_packet().await
        })
        .await
        .map_err(|_| RconError::Timeout(self.timeout_duration.as_millis() as u64))??;

        // Server returns ID=-1 on auth failure
        if response.id == -1 {
            return Err(RconError::AuthFailed);
        }

        debug!("Authentication successful");
        Ok(())
    }

    /// Send a packet to the server
    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        write_packet(&mut self.stream, packet)
            .await
            .map_err(|e| match e {
                RconError::Io(io_err) => RconError::ConnectionLost(io_err),
                other => other,
            })
    }

    /// Receive a packet from the server
    async fn receive_packet(&mut self) -> Result<Packet> {
        read_packet(&mut self.stream).await
    }

    /// Get next request ID, wrapping to stay positive and avoid -1
    fn next_request_id(&mut self) -> i32 {
        let id = self.next_id;
        // Wrap to 1 instead of overflowing or hitting -1 (auth failure sentinel)
        self.next_id = if id == i32::MAX { 1 } else { id + 1 };
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // --- Mock RCON server ---

    /// A received RCON packet (server-side view).
    struct RecvPacket {
        id: i32,
        packet_type: i32,
        payload: String,
    }

    /// Mock RCON server that reads and writes raw packets.
    /// Intentionally independent of our `Packet` type — tests the wire format.
    struct MockServer {
        stream: TcpStream,
    }

    impl MockServer {
        async fn recv(&mut self) -> RecvPacket {
            let mut len_buf = [0u8; 4];
            self.stream.read_exact(&mut len_buf).await.unwrap();
            let len = i32::from_le_bytes(len_buf) as usize;

            let mut body = vec![0u8; len];
            self.stream.read_exact(&mut body).await.unwrap();

            let id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let packet_type = i32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let payload = String::from_utf8_lossy(&body[8..len - 2]).to_string();

            RecvPacket {
                id,
                packet_type,
                payload,
            }
        }

        async fn send(&mut self, id: i32, packet_type: i32, payload: &str) {
            let payload_bytes = payload.as_bytes();
            let body_len = (4 + 4 + payload_bytes.len() + 2) as i32;

            self.stream
                .write_all(&body_len.to_le_bytes())
                .await
                .unwrap();
            self.stream.write_all(&id.to_le_bytes()).await.unwrap();
            self.stream
                .write_all(&packet_type.to_le_bytes())
                .await
                .unwrap();
            self.stream.write_all(payload_bytes).await.unwrap();
            self.stream.write_all(&[0, 0]).await.unwrap();
            self.stream.flush().await.unwrap();
        }
    }

    /// Spawn a mock RCON server. Returns the address to connect to.
    /// The handler drives the server side of the conversation.
    async fn mock_rcon<F, Fut>(handler: F) -> String
    where
        F: FnOnce(MockServer) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handler(MockServer { stream }).await;
        });
        addr
    }

    // --- Tests ---

    #[tokio::test]
    async fn auth_success() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            assert_eq!(req.packet_type, 3); // SERVERDATA_AUTH
            assert_eq!(req.payload, "secret");
            s.send(req.id, 2, "").await; // SERVERDATA_AUTH_RESPONSE
        })
        .await;

        let _client = RconClient::connect(&addr, "secret").await.unwrap();
    }

    #[tokio::test]
    async fn auth_failure() {
        let addr = mock_rcon(|mut s| async move {
            let _req = s.recv().await;
            s.send(-1, 2, "").await; // ID=-1 means auth failed
        })
        .await;

        let err = RconClient::connect(&addr, "wrong").await.unwrap_err();
        assert!(matches!(err, RconError::AuthFailed));
    }

    #[tokio::test]
    async fn execute_returns_payload() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            let req = s.recv().await;
            assert_eq!(req.packet_type, 2); // SERVERDATA_EXECCOMMAND
            assert_eq!(req.payload, "/version");
            s.send(req.id, 0, "Factorio 2.0.28").await;
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        let result = client.execute("/version").await.unwrap();
        assert_eq!(result, "Factorio 2.0.28");
    }

    #[tokio::test]
    async fn execute_empty_response() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            let req = s.recv().await;
            s.send(req.id, 0, "").await;
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        let result = client.execute("/noop").await.unwrap();
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn execute_timeout() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            let _req = s.recv().await;
            // Never respond — hold connection open
            tokio::time::sleep(Duration::from_secs(10)).await;
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        client.set_timeout(Duration::from_millis(50));

        let err = client.execute("/slow").await.unwrap_err();
        assert!(matches!(err, RconError::Timeout(_)));
    }

    #[tokio::test]
    async fn connection_lost_on_read() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            let _req = s.recv().await;
            drop(s); // Close connection before responding
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        let err = client.execute("/test").await.unwrap_err();
        assert!(matches!(err, RconError::ConnectionLost(_)));
    }

    #[tokio::test]
    async fn multiple_sequential_commands() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            for i in 1..=3 {
                let req = s.recv().await;
                s.send(req.id, 0, &format!("response {i}")).await;
            }
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        for i in 1..=3 {
            let result = client.execute(&format!("/cmd{i}")).await.unwrap();
            assert_eq!(result, format!("response {i}"));
        }
    }

    #[tokio::test]
    async fn response_id_mismatch() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            let req = s.recv().await;
            s.send(req.id + 999, 0, "wrong").await; // Wrong ID
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        let err = client.execute("/test").await.unwrap_err();
        assert!(matches!(err, RconError::ProtocolError(_)));
    }

    #[tokio::test]
    async fn request_ids_increment() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            let auth_id = req.id;
            s.send(req.id, 2, "").await;

            // Each command should have an incrementing ID
            let req = s.recv().await;
            assert_eq!(req.id, auth_id + 1);
            s.send(req.id, 0, "").await;

            let req = s.recv().await;
            assert_eq!(req.id, auth_id + 2);
            s.send(req.id, 0, "").await;
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        client.execute("/a").await.unwrap();
        client.execute("/b").await.unwrap();
    }

    #[tokio::test]
    async fn request_id_wraps_at_i32_max() {
        let addr = mock_rcon(|mut s| async move {
            let req = s.recv().await;
            s.send(req.id, 2, "").await;

            // First command should use i32::MAX
            let req = s.recv().await;
            assert_eq!(req.id, i32::MAX);
            s.send(req.id, 0, "ok1").await;

            // Second command should wrap to 1, not overflow or hit -1
            let req = s.recv().await;
            assert_eq!(req.id, 1);
            s.send(req.id, 0, "ok2").await;
        })
        .await;

        let mut client = RconClient::connect(&addr, "pass").await.unwrap();
        client.next_id = i32::MAX;

        assert_eq!(client.execute("/a").await.unwrap(), "ok1");
        assert_eq!(client.execute("/b").await.unwrap(), "ok2");
    }
}
