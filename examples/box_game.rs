use adler::Adler32;
use ggrs::{
    GGRSError, GGRSEvent, GGRSInterface, GameInput, GameState, PlayerHandle, PlayerType,
    SessionState,
};
use sdl2::render::Canvas;
use sdl2::video::Window;
use serde::{Deserialize, Serialize};
use std::env;
use std::hash::Hash;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;
const NUM_PLAYERS: usize = 2;

const INPUT_SIZE: usize = std::mem::size_of::<u32>();

const PLAYER_SIZE: u32 = 50;
const PLAYER_COLORS: [Color; 2] = [Color::RGB(0, 90, 200), Color::RGB(200, 150, 50)];

// BoxGame holds the gamestate and acts as an interface for GGRS
struct BoxGame {
    pub game_state: BoxGameState, // the game state
}

impl BoxGame {
    pub fn new() -> Self {
        BoxGame {
            game_state: BoxGameState::new(),
        }
    }
}

impl GGRSInterface for BoxGame {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let mut adler = Adler32::new();
        self.game_state.hash(&mut adler);
        let checksum = adler.checksum();
        GameState {
            frame: self.game_state.frame,
            buffer,
            checksum: Some(checksum),
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.game_state = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let _p0_inputs: u32 = bincode::deserialize(inputs[0].input()).unwrap();
        let _p1_inputs: u32 = bincode::deserialize(inputs[1].input()).unwrap();

        // increase the frame counter
        self.game_state.frame += 1;

        for i in 0..NUM_PLAYERS {
            let (old_x, old_y) = self.game_state.positions[i];
            let (vel_x, vel_y) = self.game_state.velocities[i];
            self.game_state.positions[i] = (old_x + vel_x, old_y + vel_y);
        }
    }
}

// BoxGameState holds all relevant information about the game state
#[derive(Hash, Serialize, Deserialize)]
struct BoxGameState {
    pub frame: i32,
    pub positions: Vec<(i32, i32)>,
    pub velocities: Vec<(i32, i32)>,
}

impl BoxGameState {
    pub fn new() -> Self {
        let mut positions = Vec::new();
        let mut velocities = Vec::new();
        for i in 0..NUM_PLAYERS {
            let x = WINDOW_WIDTH as i32 / 2 + (2 * i as i32 - 1) * (WINDOW_WIDTH as i32 / 4);
            let y = WINDOW_HEIGHT as i32 / 2;
            positions.push((x, y));
            velocities.push((0, 1));
        }

        Self {
            frame: 0,
            positions,
            velocities,
        }
    }
}

fn local_input() -> Vec<u8> {
    let input: u32 = 5;
    bincode::serialize(&input).unwrap()
}

fn render_frame(
    canvas: &mut Canvas<Window>,
    game: &BoxGame,
) -> Result<(), Box<dyn std::error::Error>> {
    // reset frame to be black
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();

    // draw the player rectangles
    for i in 0..NUM_PLAYERS {
        canvas.set_draw_color(PLAYER_COLORS[i]);
        let (x, y) = game.game_state.positions[i];
        let canvas_x = x - PLAYER_SIZE as i32 / 2;
        let canvas_y = y - PLAYER_SIZE as i32 / 2;
        canvas.fill_rect(Rect::new(canvas_x, canvas_y, PLAYER_SIZE, PLAYER_SIZE))?;
    }

    // flip the buffer
    canvas.present();
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments very clumsily
    let args: Vec<String> = env::args().collect();
    assert_eq!(args.len(), 4);

    let port: u16 = args[1].parse()?;
    let local_handle: PlayerHandle = args[2].parse()?;
    let remote_handle: PlayerHandle = 1 - local_handle;
    let remote_addr: SocketAddr = args[3].parse()?;

    // create a GGRS session with two players
    let mut sess = ggrs::start_p2p_session(NUM_PLAYERS as u32, INPUT_SIZE, port)?;

    // add players
    sess.add_player(PlayerType::Local, local_handle)?;
    sess.add_player(PlayerType::Remote(remote_addr), remote_handle)?;

    // start the GGRS session
    sess.start_session()?;

    // create the game
    let mut game = BoxGame::new();

    // create a window and canvas with sdl2
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let window = video_subsystem
        .window("Box Game", WINDOW_WIDTH, WINDOW_HEIGHT)
        .position_centered()
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window.into_canvas().build()?;
    let mut event_pump = sdl_context.event_pump()?;

    // start the main loop
    let mut next = Instant::now();
    let mut frames_to_skip = 0;

    'running: loop {
        // handle window events
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }

        // handle GGRS events
        for event in sess.events() {
            if let GGRSEvent::WaitRecommendation { skip_frames } = event {
                frames_to_skip += skip_frames
            }
            println!("Event: {:?}", event);
        }

        // let ggrs do some internal work
        sess.idle();

        // only process and render if it is time
        if Instant::now() < next {
            continue;
        }

        next += Duration::from_micros(16667); // 60 fps

        // do stuff only when the session is ready
        if sess.current_state() == SessionState::Running {
            // skip frames, if recommended
            if frames_to_skip > 0 {
                frames_to_skip -= 1;
            } else {
                // add local input and advance frame, if successful
                let local_input = local_input();
                match sess.add_local_input(local_handle, &local_input) {
                    Ok(()) => {
                        sess.advance_frame(&mut game)?;
                    }
                    Err(GGRSError::PredictionThreshold) => {
                        println!("PredictionThreshold reached, skipping a frame.");
                    }
                    Err(e) => {
                        return Err(Box::new(e));
                    }
                };
            }
        }

        // render the frame
        render_frame(&mut canvas, &game)?;
    }

    Ok(())
}
