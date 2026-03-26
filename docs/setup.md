# Setup

## Adding GGRS to Your Project

Add GGRS to your `Cargo.toml`:

```toml
[dependencies]
ggrs = "0.11"
```

## Feature Flags

| Feature | Description |
|---|---|
| `sync-send` | Adds `Send + Sync` bounds to `Config`, `NonBlockingSocket`, and related types. Useful when you need to share sessions across threads. |
| `wasm-bindgen` | Enables WASM support via `wasm-bindgen` and `js-sys`. Required when targeting `wasm32` with browser APIs. For WebRTC-based networking in the browser, see [Matchbox](https://github.com/johanhelsing/matchbox). |

## Requirements

### NAT Traversal

GGRS assumes you already have the socket addresses of every client you want to connect to. It does **not** handle NAT traversal or provide a signalling server. You are responsible for exchanging addresses before creating a session.

For browser-based or WebRTC networking, [Matchbox](https://github.com/johanhelsing/matchbox) provides compatible sockets and handles signalling.

### Determinism

GGRS relies on your game being **deterministic**: given the same game state and the same inputs, your game must always produce the same next state. This must hold across all participating clients, including across different CPU architectures.

Common pitfalls:

- **Floating-point arithmetic**: Results can differ between x86 and ARM, or between debug and release builds. Consider using fixed-point math or integer arithmetic for game logic.
- **HashMap/HashSet iteration order**: Non-deterministic in Rust by default. Use a deterministic map (e.g., `BTreeMap`) or ensure order is not significant.
- **Random number generators**: Seed your RNG from the game state and advance it deterministically, not from system entropy.

## Implementing the `Config` Trait

Before creating a session, implement the [`Config`](https://docs.rs/ggrs/latest/ggrs/trait.Config.html) trait to tell GGRS about your input, state, and address types:

```rust
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

pub struct GameState {
    // your game state here
}

pub struct GgrsConfig;

impl ggrs::Config for GgrsConfig {
    type Input = Input;       // transmitted over the network each frame
    type InputPredictor = ggrs::PredictRepeatLast; // how to predict missing remote inputs
    type State = GameState;   // saved/loaded during rollbacks
    type Address = std::net::SocketAddr;
}
```

`Input` must implement `Default` — GGRS uses the default value to represent "no input" for disconnected players.

### Input Prediction

When remote inputs haven't arrived yet, GGRS must predict what a player's input will be so the game can keep running without waiting. The `InputPredictor` associated type on `Config` controls this prediction strategy.

GGRS ships with two predictors:

- **`PredictRepeatLast`** — predicts that the player will repeat their last known input. This is a good default for most action games where inputs represent held state (e.g., buttons currently pressed).
- **`PredictDefault`** — always predicts `Input::default()`, regardless of the previous input. This is better suited for transition-based inputs where events are one-off (e.g., "button just pressed this frame").

You can also implement the [`InputPredictor`](https://docs.rs/ggrs/latest/ggrs/trait.InputPredictor.html) trait yourself to exploit known properties of your input format. See the rustdoc on `InputPredictor` for detailed guidance on improving prediction accuracy through input quantization and choosing between state-based and transition-based input representations.

See [Sessions](sessions.md) for the next step.
