use ggrs::SyncTestSession;
use instant::{Duration, Instant};
use macroquad::prelude::*;
use structopt::StructOpt;

const FPS: f64 = 60.0;
const MAX_PRED_FRAMES: usize = 8;

mod box_game;

/// returns a window config for macroquad to use
fn window_conf() -> Conf {
    Conf {
        window_title: "Box Game Synctest".to_owned(),
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
    num_players: usize,
    #[structopt(short, long)]
    check_distance: usize,
}

#[macroquad::main(window_conf)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();

    // create a GGRS session
    let mut sess =
        SyncTestSession::new(opt.num_players as u32, MAX_PRED_FRAMES, opt.check_distance)?;

    // set input delay for any player you want
    for i in 0..opt.num_players {
        sess.set_frame_delay(2, i)?;
    }

    // Create a new box game
    let mut game = box_game::BoxGame::new(opt.num_players);

    // time variables for tick rate
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;
    let fps_delta = 1. / FPS;

    loop {
        // get delta time from last iteration and accumulate it
        let delta = Instant::now().duration_since(last_update);
        accumulator = accumulator.saturating_add(delta);
        last_update = Instant::now();

        // if enough time is accumulated, we run a frame
        while accumulator.as_secs_f64() > fps_delta {
            // decrease accumulator
            accumulator = accumulator.saturating_sub(Duration::from_secs_f64(fps_delta));

            // gather inputs
            let mut all_inputs = Vec::new();
            for player in 0..opt.num_players {
                all_inputs.push(game.local_input(player))
            }

            match sess.advance_frame(&all_inputs) {
                Ok(requests) => game.handle_requests(requests),
                Err(e) => return Err(Box::new(e)),
            }
        }

        // render the game state
        game.render();

        // wait for the next loop (macroquads wants it so)
        next_frame().await;
    }
}
