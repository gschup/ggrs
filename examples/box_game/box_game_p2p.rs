mod box_game;

use box_game::GGRSConfig;
use ggrs::{GGRSError, P2PSession, PlayerType, SessionState, UdpNonBlockingSocket};
use instant::{Duration, Instant};
use macroquad::prelude::*;
use std::net::SocketAddr;
use structopt::StructOpt;

const FPS: f64 = 60.0;
const MAX_PRED_FRAME: usize = 8;

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
    let mut local_handle = 0;
    let num_players = opt.players.len();
    assert!(num_players > 0);

    // create a GGRS session
    let socket = UdpNonBlockingSocket::bind_to_port(opt.local_port)?;
    let mut sess = P2PSession::<GGRSConfig>::new(num_players as u32, MAX_PRED_FRAME, socket);

    // if loading/saving is more expensive than long rollbacks, you can turn on sparse saving
    // sess.set_sparse_saving(true)?;

    // set FPS (default is 60, so this doesn't change anything as is)
    sess.set_fps(FPS as u32)?;

    // add players
    for (i, player_addr) in opt.players.iter().enumerate() {
        // local player
        if player_addr == "localhost" {
            sess.add_player(PlayerType::Local, i)?;
            local_handle = i;
        } else {
            // remote players
            let remote_addr: SocketAddr = player_addr.parse()?;
            sess.add_player(PlayerType::Remote(remote_addr), i)?;
        }
    }

    // optionally, add spectators
    for (i, spec_addr) in opt.spectators.iter().enumerate() {
        sess.add_player(PlayerType::Spectator(*spec_addr), num_players + i)?;
    }

    // set input delay for the local player
    sess.set_frame_delay(2, local_handle)?;

    // set change default expected update frequency
    sess.set_fps(FPS as u32)?;

    // start the GGRS session
    sess.start_session()?;

    // Create a new box game
    let mut game = box_game::BoxGame::new(num_players);

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

        // frames are only happening if the sessions are synchronized
        if sess.current_state() == SessionState::Running {
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

                match sess.advance_frame(local_handle, game.local_input(0)) {
                    Ok(requests) => game.handle_requests(requests),
                    Err(GGRSError::PredictionThreshold) => {
                        println!("Frame {} skipped", sess.current_frame())
                    }

                    Err(e) => return Err(Box::new(e)),
                }
            }
        }

        // render the game state
        game.render();

        // wait for the next loop (macroquads wants it so)
        next_frame().await;
    }
}
