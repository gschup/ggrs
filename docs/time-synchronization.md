# Time Synchronization

In a P2P session, clients can drift apart in frame count. A faster machine — or one with lower latency — may run several frames ahead of others. When remote inputs arrive late, GGRS must roll back and re-simulate, which is expensive. Keeping clients close in frame count reduces the frequency and length of rollbacks.

GGRS provides two approaches for managing this.

## Option 1: `WaitRecommendation` Event

The simpler approach. When GGRS detects that your client has been consistently ahead by 3 or more frames for at least 60 consecutive frames, it fires a `WaitRecommendation` event:

```rust
GgrsEvent::WaitRecommendation { skip_frames } => {
    frames_to_skip += skip_frames;
}
```

In your game loop, skip that many frame advances:

```rust
if frames_to_skip > 0 {
    frames_to_skip -= 1;
    // don't call advance_frame this tick
    continue;
}
```

**Trade-off**: Skipping frames produces a visible stutter. For many games this is acceptable, but it is noticeable in fast-paced games.

## Option 2: `frames_ahead()` + Speed Adjustment

The smoother approach. `P2PSession::frames_ahead()` returns a signed integer: positive means your client is ahead, negative means you are behind. Use this to slightly adjust your frame timing rather than skipping entire frames:

```rust
let frames_ahead = session.frames_ahead();
let adjustment = if frames_ahead > 0 {
    // running ahead: slow down by 10% per frame ahead (up to some cap)
    fps_delta * (1.0 + 0.1 * frames_ahead.min(3) as f64)
} else {
    fps_delta
};

time_since_last_frame += last_tick.elapsed();
// advance only when adjusted_delta has passed
while time_since_last_frame >= adjustment {
    time_since_last_frame -= adjustment;
    // ... advance frame
}
```

By making the leading client run slightly slower, the gap closes gradually without any visible stutter.

**Trade-off**: More code to manage, but significantly smoother gameplay.

## Recommendation

For most games, the `frames_ahead()` approach produces better results. The `WaitRecommendation` approach is simpler to implement and fine for turn-based or slow-paced games where a brief pause is acceptable.

The [`ex_game_p2p`](../examples/) example demonstrates a basic implementation of both approaches.
