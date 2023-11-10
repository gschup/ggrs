mod ex_game;

use ex_game::{GGRSConfig, Game};
use ggrs::{GgrsError, GGRSEvent, SessionBuilder, SessionState, UdpNonBlockingSocket};
use instant::{Duration, Instant};
use macroquad::prelude::*;
use std::net::SocketAddr;
use structopt::StructOpt;

const FPS: f64 = 60.0;

/// returns a window config for macroquad to use
fn window_conf() -> Conf {
    Conf {
        window_title: "Box Game Spectator".to_owned(),
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
    num_players: usize,
    #[structopt(short, long)]
    host: SocketAddr,
}

#[macroquad::main(window_conf)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();

    // create a GGRS session for a spectator
    let socket = UdpNonBlockingSocket::bind_to_port(opt.local_port)?;
    let mut sess = SessionBuilder::<GGRSConfig>::new()
        .with_num_players(opt.num_players)
        .with_max_frames_behind(5)? // (optional) when the spectator is more than this amount of frames behind, it will catch up
        .with_catchup_speed(2)? // (optional) set this to 1 if you don't want any catch-ups
        .start_spectator_session(opt.host, socket);

    // Create a new box game
    let mut game = Game::new(opt.num_players);

    // time variables for tick rate
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;
    let fps_delta = 1. / FPS;

    loop {
        // communicate, receive and send packets
        sess.poll_remote_clients();

        // handle GGRS events
        for event in sess.events() {
            println!("Event: {:?}", event);
            if let GGRSEvent::Disconnected { .. } = event {
                println!("Disconnected from host.");
                return Ok(());
            }
        }

        // get delta time from last iteration and accumulate it
        let delta = Instant::now().duration_since(last_update);
        accumulator = accumulator.saturating_add(delta);
        last_update = Instant::now();

        // if enough time is accumulated, we run a frame
        while accumulator.as_secs_f64() > fps_delta {
            // decrease accumulator
            accumulator = accumulator.saturating_sub(Duration::from_secs_f64(fps_delta));

            // execute a frame
            if sess.current_state() == SessionState::Running {
                match sess.advance_frame() {
                    Ok(requests) => game.handle_requests(requests),
                    Err(GgrsError::PredictionThreshold) => {
                        println!(
                            "Frame {} skipped: Waiting for input from host.",
                            game.current_frame()
                        );
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
