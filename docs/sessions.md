# Sessions

GGRS provides three session types. All are constructed with [`SessionBuilder`](https://docs.rs/ggrs/latest/ggrs/struct.SessionBuilder.html).

## Session Types

### `P2PSession`

The main session type for multiplayer games. All participating clients create their own `P2PSession` and connect to each other in a peer-to-peer mesh. Each client sends only its own local inputs; GGRS handles prediction and rollback transparently.

### `SpectatorSession`

Connects to an existing host running a `P2PSession`. The host broadcasts all confirmed inputs to the spectator, who can reproduce the game state locally without contributing any input. The spectator does not affect the game.

### `SyncTestSession`

A local-only session for testing determinism. On every frame, GGRS simulates a rollback and re-runs the last *n* frames (where *n* is the check distance), then compares checksums. No network is involved. Use this during development to verify that your save/load/advance logic is correct and deterministic.

---

## Building a Session

All sessions are created through `SessionBuilder`. The builder is consumed by the `start_*` methods.

### P2P Session

```rust
use ggrs::{SessionBuilder, PlayerType, UdpNonBlockingSocket};

let socket = UdpNonBlockingSocket::bind_to_port(7000)
    .expect("failed to bind socket");

let mut session = SessionBuilder::<GgrsConfig>::new()
    .with_num_players(2)?
    .with_fps(60)?
    .with_input_delay(2)
    .add_player(PlayerType::Local, 0)?
    .add_player(PlayerType::Remote("127.0.0.1:7001".parse()?), 1)?
    .start_p2p_session(socket)?;
```

### Spectator Session

```rust
let socket = UdpNonBlockingSocket::bind_to_port(7002)
    .expect("failed to bind socket");

let host_addr: std::net::SocketAddr = "127.0.0.1:7000".parse()?;

let mut session = SessionBuilder::<GgrsConfig>::new()
    .with_num_players(2)?
    .start_spectator_session(host_addr, socket);
```

### SyncTest Session

```rust
let mut session = SessionBuilder::<GgrsConfig>::new()
    .with_num_players(2)?
    .with_check_distance(7)
    .start_synctest_session()?;
```

---

## Common Builder Options

| Method | Default | Description |
|---|---|---|
| `with_num_players(n)` | 2 | Total number of players (not counting spectators). |
| `with_fps(fps)` | 60 | Expected update frequency. Used for frame synchronization heuristics. |
| `with_input_delay(n)` | 0 | Frames of artificial delay applied to local input. Reduces rollbacks at the cost of added latency. |
| `with_max_prediction_window(n)` | 8 | Maximum frames GGRS will predict ahead. Set to `0` for lockstep mode (no rollbacks, no prediction). |
| `with_sparse_saving_mode(bool)` | false | Only save state at the last confirmed frame. See [Sparse Saving](sparse-saving.md). |
| `with_desync_detection_mode(mode)` | Off | Enable checksum-based desync detection. See [`DesyncDetection`](https://docs.rs/ggrs/latest/ggrs/enum.DesyncDetection.html). |
| `with_disconnect_timeout(duration)` | 2s | How long without packets before a remote peer is disconnected. |
| `with_disconnect_notify_delay(duration)` | 500ms | How long before a `NetworkInterrupted` event is sent. |

---

## Multiple Local Players

GGRS supports multiple players on the same client (e.g., two gamepads on one machine). Add multiple `PlayerType::Local` entries with different handles:

```rust
let session = SessionBuilder::<GgrsConfig>::new()
    .with_num_players(3)?
    .add_player(PlayerType::Local, 0)?           // local player 1
    .add_player(PlayerType::Local, 1)?           // local player 2 (same machine)
    .add_player(PlayerType::Remote(remote_addr), 2)?
    .start_p2p_session(socket)?;
```

Call `add_local_input` once per local handle before calling `advance_frame`.

---

## Session State

After construction, a `P2PSession` starts in `SessionState::Synchronizing`. During this phase, GGRS exchanges sync packets with remote peers. Once synchronized, the session moves to `SessionState::Running` and begins accepting inputs.

```rust
if session.current_state() == ggrs::SessionState::Running {
    // safe to advance frames
}
```

See [Main Loop](main-loop.md) for how to structure your game loop around this.
