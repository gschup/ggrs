use instant::{Duration, Instant};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::{collections::hash_map::DefaultHasher, net::SocketAddr};
use structopt::StructOpt;

use ggrs::{
    Frame, GGRSError, GGRSRequest, GameInput, GameState, GameStateCell, P2PSession, PlayerType,
    SessionState,
};

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

// this is to read command-line arguments
#[derive(StructOpt)]
struct Opt {
    #[structopt(short, long)]
    local_port: u16,
    #[structopt(short, long)]
    players: Vec<String>,
}

// some constants for the app
const FPS: f64 = 60.0;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();

#[macroquad::main("Tug-of-War")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();
    let mut local_handle = 0;
    let num_players = opt.players.len();
    let max_pred_frames = 8;
    assert!(num_players == 2); // this example is only for two p2p players

    // create a GGRS session
    let mut sess = P2PSession::new(
        num_players as u32,
        INPUT_SIZE,
        max_pred_frames,
        opt.local_port,
    )?;

    // set FPS (default is 60, so this doesn't change anything as is)
    sess.set_fps(FPS as u32)?;

    // add players
    for (i, player_addr) in opt.players.iter().enumerate() {
        // local player
        if player_addr == "localhost" {
            sess.add_player(PlayerType::Local, i)?;
            local_handle = i;
        } else {
            // remote player
            let remote_addr: SocketAddr = player_addr.parse()?;
            sess.add_player(PlayerType::Remote(remote_addr), i)?;
        }
    }

    // set change default expected update frequency
    sess.set_fps(FPS as u32)?;

    // start the GGRS session
    sess.start_session()?;

    // time variables for tick rate
    let mut last_update = Instant::now();
    let mut accumulator = Duration::ZERO;

    // setup the "game"
    let mut game = TugOfWarGame::new();

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

                match sess.advance_frame(local_handle, &local_input()) {
                    Ok(requests) => game.handle_requests(requests),
                    Err(GGRSError::PredictionThreshold) => println!("Frame skipped"),
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

#[derive(Copy, Clone, Hash, Serialize, Deserialize)]
struct TowState {
    frame: i32,
    x: i32,
}

impl TowState {
    fn new() -> Self {
        Self { frame: 0, x: 0 }
    }
}
struct TugOfWarGame {
    state: TowState,
}

impl TugOfWarGame {
    fn new() -> Self {
        Self {
            state: TowState::new(),
        }
    }

    fn render(&self) {
        clear_background(BLACK);
        let x_on_screen = screen_width() / 2.0 + self.state.x as f32;
        draw_circle(x_on_screen, screen_height() / 2.0, 15.0, YELLOW);
    }

    // handle all GGRS requests
    fn handle_requests(&mut self, requests: Vec<GGRSRequest<TowState>>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.state.frame += 1;

        let p1_pressed = inputs[0].buffer[0] > 0;
        let p2_pressed = inputs[0].buffer[0] > 0;

        if p1_pressed {
            self.state.x += 2;
        }

        if p2_pressed {
            self.state.x -= 2;
        }
    }

    // save current gamestate, create a checksum
    // creating a checksum here is only relevant for SyncTestSessions
    fn save_game_state(&mut self, cell: GameStateCell<TowState>, frame: Frame) {
        assert_eq!(self.state.frame, frame);
        let checksum = calculate_hash(&self.state);

        cell.save(GameState::new_with_checksum(
            frame,
            Some(self.state),
            checksum,
        ));
    }

    // load and overwrite current gamestate
    fn load_game_state(&mut self, cell: GameStateCell<TowState>) {
        let state_to_load = cell.load();
        self.state = state_to_load.data.expect("No data found.");
    }
}

// in this example, there is only the space bar
fn local_input() -> Vec<u8> {
    if is_key_down(KeyCode::Space) {
        vec![0]
    } else {
        vec![1]
    }
}
