mod ex_game;

use ex_game::{GGRSConfig, Game};
use ggrs::{PlayerType, SessionBuilder, SessionState, UdpNonBlockingSocket};
use instant::{Duration, Instant};
use macroquad::prelude::*;
use std::net::SocketAddr;
use structopt::StructOpt;

const FPS: f64 = 60.0;

/// returns a window config for macroquad to use
fn window_conf() -> Conf {
    Conf {
        window_title: "Box Game P2P".to_owned(),
        window_width: 600,
        window_height: 800,
        window_resizable: false,
        high_dpi: true,
        ..Default::default()
    }
}

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, long)]
    local_port: u16,
    #[structopt(short, long)]
    players: Vec<String>,
    #[structopt(short, long)]
    spectators: Vec<SocketAddr>,
}

#[macroquad::main(window_conf)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();
    let num_players = opt.players.len();
    assert!(num_players > 0);

    // create a GGRS session
    let mut sess_build = SessionBuilder::<GGRSConfig>::new()
        .with_num_players(num_players)
        // (optional) exchange and validate state checksums
        .with_desync_detection_mode(ggrs::DesyncDetection::On { interval: 100 })
        // (optional) set expected update frequency
        .with_fps(FPS as usize)?
        // (optional) customize prediction window, which is how many frames ahead GGRS predicts.
        // Or set the prediction window to 0 to use lockstep netcode instead (i.e. no rollbacks).
        .with_max_prediction_window(8)
        // (optional) set input delay for the local player
        .with_input_delay(2)
        // (optional) by default, GGRS will ask you to save the game state every frame. If your
        // saving of game state takes much longer than advancing the game state N times, you can
        // improve performance by turning sparse saving mode on (N == average number of predictions
        // GGRS must make, which is determined by prediction window, FPS and latency to clients).
        .with_sparse_saving_mode(false);

    // add players
    for (i, player_addr) in opt.players.iter().enumerate() {
        // local player
        if player_addr == "localhost" {
            sess_build = sess_build.add_player(PlayerType::Local, i)?;
        } else {
            // remote players
            let remote_addr: SocketAddr = player_addr.parse()?;
            sess_build = sess_build.add_player(PlayerType::Remote(remote_addr), i)?;
        }
    }

    // optionally, add spectators
    for (i, spec_addr) in opt.spectators.iter().enumerate() {
        sess_build = sess_build.add_player(PlayerType::Spectator(*spec_addr), num_players + i)?;
    }

    // start the GGRS session
    let socket = UdpNonBlockingSocket::bind_to_port(opt.local_port)?;
    let mut sess = sess_build.start_p2p_session(socket)?;

    // Create a new box game
    let mut game = Game::new(num_players);
    game.register_local_handles(sess.local_player_handles());

    // time variables for tick rate
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;

    loop {
        // communicate, receive and send packets
        sess.poll_remote_clients();

        // print GGRS events
        for event in sess.events() {
            println!("Event: {:?}", event);
        }

        // this is to keep ticks between clients synchronized.
        // if a client is ahead, it will run frames slightly slower to allow catching up
        let mut fps_delta = 1. / FPS;
        if sess.frames_ahead() > 0 {
            fps_delta *= 1.1;
        }

        // get delta time from last iteration and accumulate it
        let delta = Instant::now().duration_since(last_update);
        accumulator = accumulator.saturating_add(delta);
        last_update = Instant::now();

        // if enough time is accumulated, we run a frame
        while accumulator.as_secs_f64() > fps_delta {
            // decrease accumulator
            accumulator = accumulator.saturating_sub(Duration::from_secs_f64(fps_delta));

            // frames are only happening if the sessions are synchronized
            if sess.current_state() == SessionState::Running {
                // add input for all local  players
                for handle in sess.local_player_handles() {
                    sess.add_local_input(handle, game.local_input(handle))?;
                }

                match sess.advance_frame() {
                    Ok(requests) => game.handle_requests(requests, sess.in_lockstep_mode()),
                    Err(e) => return Err(Box::new(e)),
                }
            }
        }

        // render the game state
        game.render();

        // wait for the next loop (macroquad wants it so)
        next_frame().await;
    }
}
