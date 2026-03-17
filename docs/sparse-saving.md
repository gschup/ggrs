# Sparse Saving

## Default Behavior

By default, GGRS saves your game state on every frame advance. This ensures that any rollback of up to `max_prediction_window` frames has a nearby save point to restore from.

For a prediction window of 8, GGRS keeps up to 9 save slots (current frame + 8 predicted). Each `AdvanceFrame` request is preceded by a `SaveGameState` request.

## What Sparse Saving Does

With sparse saving enabled, GGRS only saves at the **last confirmed frame** — the most recent frame for which all clients have provided real (non-predicted) inputs. This means:

- Fewer `SaveGameState` requests: at most one per update tick instead of one per frame.
- Potentially longer rollbacks: if a misprediction is detected, GGRS must re-simulate from the last confirmed save rather than a closer frame.

## When to Use It

Sparse saving is beneficial when **saving state is expensive** (e.g., large game states, or serialization to a buffer). If your state save is cheap and fast, sparse saving is unlikely to help and may increase the average rollback cost.

Measure both approaches in your game before committing to either.

## Enabling Sparse Saving

Configure it via `SessionBuilder` before starting a session:

```rust
let session = SessionBuilder::<GgrsConfig>::new()
    .with_num_players(2)?
    .with_sparse_saving_mode(true)
    // ... other options
    .start_p2p_session(socket)?;
```

No other changes to your request handling are needed — the `SaveGameState` and `LoadGameState` requests work identically either way.

## Incompatibility with `SyncTestSession`

Sparse saving cannot be used with `SyncTestSession`. The sync test must save every frame to have checksums available across the full check window. Calling `start_synctest_session()` with sparse saving enabled returns `GgrsError::InvalidRequest`.
