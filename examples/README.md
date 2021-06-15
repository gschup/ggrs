# Example Instructions

Gathered here are some additional instructions on how to build and run the examples.

## BoxGame
BoxGame is a very basic two-player example with each player controlling a coloured box. There is no real game, just movement with slight ice physics. BoxGame uses [rust bindings](https://github.com/Rust-SDL2/rust-sdl2) of [SDL2](https://www.libsdl.org/) in order to launch and render a window. The bindings come bundled with binaries for SDL2, but depending on your machine, you might need to install SDL2 yourself first.

WARNING: Currently, a spectator connecting to host 127.0.0.1:7000 and listening on 127.0.0.1:7002 is hard-coded for testing purposes. WITHOUT STARTING A SPECTATOR LIKE SHOWN BELOW, THE SESSIONS WILL NOT START!

The example is properly launched by command-line arguments:
```
cargo run --example box_game -- local_port local_player_handle remote_adress:remote_port
```


To run two instances of the game and a spectator on your local machine, run these commands in separate terminals:
```
cargo run --example box_game -- 7000 0 127.0.0.1:7001 
cargo run --example box_game -- 7001 1 127.0.0.1:7000 
cargo run --example box_game_spectator -- 7002 127.0.0.1:7000 
```
