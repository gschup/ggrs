# GGRS - P2P Rollback Networking in Rust

[![crates.io](https://img.shields.io/crates/v/ggrs?style=for-the-badge)](https://crates.io/crates/ggrs)
[![docs.rs](https://img.shields.io/docsrs/ggrs?style=for-the-badge)](https://docs.rs/ggrs/newest/ggrs/)
![GitHub Workflow Status](https://img.shields.io/github/workflow/status/gschup/ggrs/Rust?style=for-the-badge)
![GitHub top language](https://img.shields.io/github/languages/top/gschup/ggrs?style=for-the-badge)
![GitHub](https://img.shields.io/github/license/gschup/ggrs?style=for-the-badge)

GGRS (good game rollback system) is a reimagination of the [GGPO network SDK](https://www.ggpo.net/) written in 100% safe [Rust 🦀](https://www.rust-lang.org/). The callback-style API from the original library has been replaced with a much saner, simpler control flow. Instead of registering callback functions, GGRS returns a list of requests for the user to fulfill.

For now, take a look at [the documentation](https://docs.rs/ggrs/newest/ggrs/) or the examples in order to check it out!

If you are interested in integrating rollback networking into your game or just want to chat with other rollback developers (not limited to Rust), check out the [GGPO Developers Discord](https://discord.com/invite/8FKKhCRCCE)!

## Development Status

GGRS is in an early stage, but the main functionality for two players and spectators should be quite stable.

## What is GGPO / Rollback?

Taken from [the official GGPO website](https://ggpo.net/):

>Rollback networking is designed to be integrated into a fully deterministic peer-to-peer engine.  With full determinism, the game is guaranteed to play out the same way on all players computers if we simply feed them the same inputs.  One way to achieve this is to exchange inputs for all players over the network, only execution a frame of gameplay logic when all players have received all the inputs from their peers.  This often results in sluggish, unresponsive gameplay.  The longer it takes to get inputs over the network, the slower the game becomes.

>In rollback networking, game logic is allowed to proceed with just the inputs from the local player.  If the remote inputs have not yet arrived when it's time to execute a frame, the networking code will predict what it expects the remote players to do based on previously seen inputs.  Since there's no waiting, the game feels just as responsive as it does offline.  When those inputs finally arrive over the network, they can be compared to the ones that were predicted earlier.  If they differ, the game can be re-simulated from the point of divergence to the current visible frame.

>Don't worry if that sounds like a headache.  GGPO was designed specifically to implement the rollback algorithms and low-level networking logic in a way that's easy to integrate into your existing game loop.  If you simply implement the functionality to save your game state, load it back up, and execute a frame of game state without rendering its outcome, GGPO can take care of the rest.

For more information about GGPO, check out [the official website](http://ggpo.net/) or [the official github repository](https://github.com/pond3r/ggpo). A very good pseudocode explanation for general rollback networking can be found [in this Gist](https://gist.github.com/rcmagic/f8d76bca32b5609e85ab156db38387e9).

## Other Rollback Implementations in Rust

Also take a look at the awesome [backroll-rs](https://github.com/HouraiTeahouse/backroll-rs/)!

## Licensing

Just like the original GGPO, GGRS is available under The MIT License. This means GGRS is free for commercial and non-commercial use. Attribution is not required, but appreciated.
