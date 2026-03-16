# Main Loop

GGRS is driven by your game loop. Each iteration you poll the network, handle events, and — when enough time has elapsed — advance the frame.

## The Basic Pattern

```
loop:
  1. poll_remote_clients()       — receive/send UDP packets
  2. drain events()              — handle GgrsEvents
  3. check frames_ahead()        — slow down if running ahead
  4. accumulate delta time
  5. if enough time has passed:
       add_local_input(handle, input)   — for each local player
       advance_frame()                  — returns Vec<GgrsRequest>
       handle each request in order
  6. render
```

## Polling

`poll_remote_clients()` receives incoming UDP packets and dispatches any queued outgoing packets. Call it every iteration, regardless of whether you advance a frame that tick:

```rust
session.poll_remote_clients();
```

## Events

After polling, drain the event queue. Most events are informational (connection status, desync detection), but `WaitRecommendation` requires action:

```rust
for event in session.events() {
    match event {
        GgrsEvent::Synchronizing { addr, total, count } => { /* show progress */ }
        GgrsEvent::Synchronized { addr } => { /* peer connected */ }
        GgrsEvent::Disconnected { addr } => { /* handle disconnect */ }
        GgrsEvent::NetworkInterrupted { addr, disconnect_timeout } => { /* warn user */ }
        GgrsEvent::NetworkResumed { addr } => { /* connection restored */ }
        GgrsEvent::WaitRecommendation { skip_frames } => {
            // your client is running ahead; skip this many frames
            frames_to_skip += skip_frames;
        }
        GgrsEvent::DesyncDetected { frame, local_checksum, remote_checksum, addr } => {
            // checksums diverged — your game has a determinism bug
        }
    }
}
```

See [Requests and Events](requests-and-events.md) for more detail.

## Time Accumulation

GGRS does not control your frame timing — you advance frames at whatever rate your loop runs. A fixed-timestep accumulator is the standard approach:

```rust
let fps_delta = Duration::from_secs_f64(1.0 / TARGET_FPS);
time_since_last_frame += last_tick.elapsed();
last_tick = Instant::now();

while time_since_last_frame >= fps_delta {
    time_since_last_frame -= fps_delta;

    // add input for each local player handle
    session.add_local_input(local_handle, current_input)?;

    match session.advance_frame() {
        Ok(requests) => {
            for request in requests {
                handle_ggrs_request(request);
            }
        }
        Err(GgrsError::PredictionThreshold) => {
            // remote peer is too far behind; skip this frame
        }
        Err(e) => return Err(e),
    }
}
```

## Time Synchronization

When your client runs ahead of remote peers, you accumulate one-sided rollbacks. GGRS provides two tools to compensate:

- **`WaitRecommendation` event**: Simple. GGRS fires this when it detects you're consistently ahead by 3+ frames for 60 consecutive frames. Skip the recommended number of frames.
- **`frames_ahead()`**: More precise. Returns how many frames ahead (positive) or behind (negative) your session is. You can use this to slightly widen or narrow your frame delta to drift back into sync without skipping.

See [Time Synchronization](time-synchronization.md) for details on each approach.

## Synchronization Phase

During `SessionState::Synchronizing`, `advance_frame()` returns `Err(GgrsError::NotSynchronized)`. You can either check `current_state()` before calling, or simply treat `NotSynchronized` as a non-fatal skip:

```rust
match session.advance_frame() {
    Ok(requests) => { /* handle requests */ }
    Err(GgrsError::NotSynchronized) | Err(GgrsError::PredictionThreshold) => {
        // not ready or too far ahead — skip this tick
    }
    Err(e) => return Err(e),
}
```

## Working Example

The [`ex_game_p2p`](../examples/) example in this repository demonstrates a complete working game loop with P2P, spectator, and sync-test modes.
