start cargo run --example box_game_p2p -- 7000 0 127.0.0.1:7001 127.0.0.1:7002
start cargo run --example box_game_p2p -- 7001 1 127.0.0.1:7000 
start cargo run --example box_game_spectator -- 7002 127.0.0.1:7000 