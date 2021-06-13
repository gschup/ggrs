# Example Instructions

Gathered here are some additional instructions on how to build and run the examples.

## BoxGame
BoxGame is a very basic two-player example with each player controlling a coloured box. There is no real game, just movement with slight ice physics. The example is properly launched by command-line arguments:
```
cargo run --example box_game -- local_port local_player_handle remote_adress:remote_port
```


To run two instances of the game on your local machine, run these commands separately:
```
cargo run --example box_game -- 7000 0 127.0.0.1:7001 
cargo run --example box_game -- 7001 1 127.0.0.1:7000 
```
