# Example Instructions

Gathered here are some additional instructions on how to build and run the examples.

## BoxGame
BoxGame is a very basic two-player example with each player controlling a coloured box. 
There is no real game, just movement with slight ice physics. Optionally, 
you can specify one spectator. BoxGame uses 
[rust bindings](https://github.com/Rust-SDL2/rust-sdl2) of [SDL2](https://www.libsdl.org/) in 
order to launch and render a window. The bindings come bundled with binaries for SDL2, but 
depending on your machine, you might need to install SDL2 yourself first.

An emergent side effect of my shoddy window handling: You can simulate network interruptions by 
dragging and holding the window in order to stop it from processing events.

### Launching BoxGame P2P and Spectator
The example is properly launched by command-line arguments 
(with the spectator address in brackets being optional):
```
cargo run --example box_game -- local_port local_player_handle remote_adress [spectator_address]
```

To run two instances of the game and a spectator on your local machine, 
run these commands in separate terminals:
```
cargo run --example box_game -- 7000 0 127.0.0.1:7001 127.0.0.1:7002
cargo run --example box_game -- 7001 1 127.0.0.1:7000 
cargo run --example box_game_spectator -- 7002 127.0.0.1:7000 
```

## BoxGame SyncTest
The same game, but without network functionality. 
Instead, the SyncTestSession focusses on simulating rollbacks and comparing checksums that you
should provide. If you do not provide checksums, SyncTestSession does nearly nothing.

### Launching BoxGame SyncTest
```
cargo run --example box_game_synctest
```