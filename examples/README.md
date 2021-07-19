# Example Instructions

Gathered here are some additional instructions on how to build and run the examples.

## BoxGame

BoxGame is a very basic two-player example with each player controlling a coloured box.
There is no real game, just movement with ice physics. Optionally,
you can specify one spectator.

- W to accelerate forwards
- S to accelerate backwards
- A to turn left
- D to turn right

An emergent side effect of my shoddy window handling: You can simulate network interruptions by
dragging and holding the window in order to stop it from processing events.

### Launching BoxGame P2P and Spectator

The example is properly launched by command-line arguments
(with the spectator address in brackets being optional):

```shell
cargo run --example box_game_p2p -- local_port local_player_handle remote_adress [spectator_address]
cargo run --example box_game_spectator -- local_port host_adress
```

To run two instances of the game and a spectator on your local machine,
run these commands in separate terminals:

```shell
cargo run --example box_game_p2p -- 7000 0 127.0.0.1:7001 127.0.0.1:7002
cargo run --example box_game_p2p -- 7001 1 127.0.0.1:7000 
cargo run --example box_game_spectator -- 7002 127.0.0.1:7000 
```

## BoxGame SyncTest

The same game, but without network functionality.
Instead, the SyncTestSession focusses on simulating rollbacks and comparing checksums that you
should provide. If you do not provide checksums, SyncTestSession does nearly nothing.

### Launching BoxGame SyncTest

```shell
cargo run --example box_game_synctest
```
