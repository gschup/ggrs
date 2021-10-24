use instant::{Duration, Instant};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use structopt::StructOpt;

use ggrs::{
    Frame, GGRSError, GGRSRequest, GameInput, GameState, GameStateCell, P2PSession, PlayerType,
    SessionState,
};

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
    assert!(num_players == 2); // this example is only for two p2p players

    // create a GGRS session
    let mut sess = P2PSession::new(num_players as u32, INPUT_SIZE, opt.local_port)?;

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

#[derive(Serialize, Deserialize)]
struct TugofWarGameState {
    frame: i32,
    x: i32,
}

impl TugofWarGameState {
    fn new() -> Self {
        Self { frame: 0, x: 0 }
    }
}
struct TugOfWarGame {
    state: TugofWarGameState,
}

impl TugOfWarGame {
    fn new() -> Self {
        Self {
            state: TugofWarGameState::new(),
        }
    }

    fn render(&self) {
        clear_background(BLACK);
        let x_on_screen = screen_width() / 2.0 + self.state.x as f32;
        draw_circle(x_on_screen, screen_height() / 2.0, 15.0, YELLOW);
    }

    // handle all GGRS requests
    fn handle_requests(&mut self, requests: Vec<GGRSRequest>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.state.frame += 1;

        let p1_pressed = inputs[0].input()[0] > 0;
        let p2_pressed = inputs[1].input()[0] > 0;

        if p1_pressed {
            self.state.x += 2;
        }

        if p2_pressed {
            self.state.x -= 2;
        }
    }

    // serialize current gamestate, create a checksum
    // creating a checksum here is only relevant for SyncTestSessions
    fn save_game_state(&mut self, cell: GameStateCell, frame: Frame) {
        assert_eq!(self.state.frame, frame);
        let buffer = bincode::serialize(&self.state).unwrap();
        let checksum = fletcher16(&buffer) as u64;

        cell.save(GameState::new(frame, Some(buffer), Some(checksum)));
    }

    // deserialize gamestate to load and overwrite current gamestate
    fn load_game_state(&mut self, cell: GameStateCell) {
        let state_to_load = cell.load();
        self.state = bincode::deserialize(&state_to_load.buffer.unwrap()).unwrap();
    }
}

/// computes the fletcher16 checksum, copied from wikipedia: <https://en.wikipedia.org/wiki/Fletcher%27s_checksum>
fn fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;

    for index in 0..data.len() {
        sum1 = (sum1 + data[index] as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }

    (sum2 << 8) | sum1
}

// in this example, there is only the space bar
fn local_input() -> Vec<u8> {
    if is_key_down(KeyCode::Space) {
        vec![0]
    } else {
        vec![1]
    }
}
