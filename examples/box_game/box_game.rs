extern crate freetype as ft;

use ft::Library;
use ggrs::{
    Frame, GGRSEvent, GGRSRequest, GameInput, GameState, GameStateCell, PlayerHandle, PlayerType,
    SessionState, NULL_FRAME,
};
use glutin_window::GlutinWindow as Window;
use graphics::{Context, Graphics, ImageSize};
use opengl_graphics::{GlGraphics, OpenGL, Texture, TextureSettings};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderArgs, RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{Button, EventLoop, IdleEvent, Key, PressEvent, ReleaseEvent};
use serde::{Deserialize, Serialize};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;

const FPS: u64 = 60;
const NUM_PLAYERS: usize = 2;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();
const CHECKSUM_PERIOD: i32 = 100;

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.35, 0.78, 1.0];
const ORANGE: [f32; 4] = [0.78, 0.59, 0.2, 1.0];
const PLAYER_COLORS: [[f32; 4]; 2] = [BLUE, ORANGE];

const PLAYER_SIZE: f64 = 50.0;
const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

const INPUT_UP: u8 = 1 << 0;
const INPUT_DOWN: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;

const PLAYER_SPEED: i32 = 240;

/// Computes the fletcher16 checksum, copied from wikipedia: https://en.wikipedia.org/wiki/Fletcher%27s_checksum
fn fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;

    for index in 0..data.len() {
        sum1 = (sum1 + data[index] as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }

    return (sum2 << 8) | sum1;
}

fn glyphs(face: &mut ft::Face, text: &str) -> Vec<(Texture, [f64; 2])> {
    let mut x = 10;
    let mut y = 0;
    let mut res = vec![];
    for ch in text.chars() {
        face.load_char(ch as usize, ft::face::LoadFlag::RENDER)
            .unwrap();
        let g = face.glyph();

        let bitmap = g.bitmap();
        let texture = Texture::from_memory_alpha(
            bitmap.buffer(),
            bitmap.width() as u32,
            bitmap.rows() as u32,
            &TextureSettings::new(),
        )
        .unwrap();
        res.push((
            texture,
            [(x + g.bitmap_left()) as f64, (y - g.bitmap_top()) as f64],
        ));

        x += (g.advance().x >> 6) as i32;
        y += (g.advance().y >> 6) as i32;
    }
    res
}

fn render_text<G, T>(glyphs: &[(T, [f64; 2])], c: &Context, gl: &mut G)
where
    G: Graphics<Texture = T>,
    T: ImageSize,
{
    for &(ref texture, [x, y]) in glyphs {
        use graphics::*;

        Image::new_color(color::WHITE).draw(texture, &c.draw_state, c.transform.trans(x, y), gl);
    }
}

pub struct BoxGame {
    game_state: BoxGameState,
    wasd_pressed: [bool; 4],
    font: PathBuf,
    last_checksum: (Frame, u64),
    periodic_checksum: (Frame, u64),
}

impl BoxGame {
    pub fn new(font: PathBuf) -> Self {
        Self {
            game_state: BoxGameState::new(),
            wasd_pressed: [false; 4],
            font,
            last_checksum: (NULL_FRAME, 0),
            periodic_checksum: (NULL_FRAME, 0),
        }
    }

    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell, frame: Frame) {
        assert_eq!(self.game_state.frame, frame);
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let checksum = fletcher16(&buffer) as u64;

        // remember checksum to render it later
        self.last_checksum = (frame, checksum);
        if frame % CHECKSUM_PERIOD == 0 {
            self.periodic_checksum = (frame, checksum);
        }

        cell.save(GameState::new(frame, Some(buffer), Some(checksum)));
    }

    fn load_game_state(&mut self, cell: GameStateCell) {
        let state_to_load = cell.load();
        self.game_state = bincode::deserialize(&state_to_load.buffer.unwrap()).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        // increase the frame counter
        self.game_state.frame += 1;

        for i in 0..NUM_PLAYERS {
            // get input of that player
            let input;
            // check if the player is disconnected (disconnected players might maybe do something different)
            if inputs[i].frame == NULL_FRAME {
                input = 0; // disconnected players do nothing
            } else {
                input = bincode::deserialize(inputs[i].input()).unwrap();
            }

            // old values
            let (old_x, old_y) = self.game_state.positions[i];
            let (old_vel_x, old_vel_y) = self.game_state.velocities[i];
            // slow down
            let mut vel_x = (9 * old_vel_x) / 10;
            let mut vel_y = (9 * old_vel_y) / 10;
            // apply input
            if input & INPUT_UP != 0 {
                vel_y = -PLAYER_SPEED;
            }
            if input & INPUT_DOWN != 0 {
                vel_y = PLAYER_SPEED;
            }
            if input & INPUT_LEFT != 0 {
                vel_x = -PLAYER_SPEED;
            }
            if input & INPUT_RIGHT != 0 {
                vel_x = PLAYER_SPEED;
            }
            // compute new values
            let mut x = old_x + vel_x / FPS as i32;
            let mut y = old_y + vel_y / FPS as i32;

            //constrain boxes to canvas borders
            x = std::cmp::max(x, 0);
            x = std::cmp::min(x, WINDOW_WIDTH as i32 - PLAYER_SIZE as i32);
            y = std::cmp::max(y, 0);
            y = std::cmp::min(y, WINDOW_HEIGHT as i32 - PLAYER_SIZE as i32);

            self.game_state.positions[i] = (x as i32, y as i32);
            self.game_state.velocities[i] = (vel_x as i32, vel_y as i32);
        }
    }

    fn render(&mut self, gl: &mut GlGraphics, freetype: &Library, args: &RenderArgs) {
        use graphics::*;

        let mut face = freetype.new_face(&self.font, 0).unwrap();
        face.set_pixel_sizes(0, 40).unwrap();
        let checksum_string = format!(
            "Frame {}: Checksum {}",
            self.last_checksum.0, self.last_checksum.1
        );
        let checksum_glyphs = glyphs(&mut face, &checksum_string);
        let periodic_string = format!(
            "Frame {}: Checksum {}",
            self.periodic_checksum.0, self.periodic_checksum.1
        );
        let periodic_glyphs = glyphs(&mut face, &periodic_string);

        gl.draw(args.viewport(), |c, gl| {
            // Clear the screen.
            clear(BLACK, gl);
            render_text(&checksum_glyphs, &c.trans(0.0, 40.0), gl);
            render_text(&periodic_glyphs, &c.trans(0.0, 80.0), gl);

            // draw the player rectangles
            for i in 0..NUM_PLAYERS {
                let square = rectangle::square(0.0, 0.0, PLAYER_SIZE);
                let rotation = 0.0;
                let (x, y) = self.game_state.positions[i];

                let transform = c.transform.trans(x as f64, y as f64).rot_rad(rotation);
                rectangle(PLAYER_COLORS[i], square, transform, gl);
            }
        });
    }

    fn local_input(&self) -> Vec<u8> {
        // Create a set of pressed Keys.
        let mut input: u8 = 0;

        // ugly, but it works...
        if self.wasd_pressed[0] {
            input |= INPUT_UP;
        }
        if self.wasd_pressed[1] {
            input |= INPUT_LEFT;
        }
        if self.wasd_pressed[2] {
            input |= INPUT_DOWN;
        }
        if self.wasd_pressed[3] {
            input |= INPUT_RIGHT;
        }

        bincode::serialize(&input).unwrap()
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
            velocities.push((0, 0));
        }

        Self {
            frame: 0,
            positions,
            velocities,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments very clumsily
    let args: Vec<String> = env::args().collect();
    assert!(args.len() >= 4);

    let port: u16 = args[1].parse()?;
    let local_handle: PlayerHandle = args[2].parse()?;
    let remote_handle: PlayerHandle = 1 - local_handle;
    let remote_addr: SocketAddr = args[3].parse()?;

    // create a GGRS session with two players
    let mut sess = ggrs::start_p2p_session(NUM_PLAYERS as u32, INPUT_SIZE, port)?;

    // add players
    sess.add_player(PlayerType::Local, local_handle)?;
    sess.add_player(PlayerType::Remote(remote_addr), remote_handle)?;

    // optionally, add a spectator
    if args.len() > 4 {
        let spec_addr: SocketAddr = args[4].parse()?;
        sess.add_player(PlayerType::Spectator(spec_addr), 2)?;
    }

    // set input delay for the local player
    sess.set_frame_delay(2, local_handle)?;

    // start the GGRS session
    sess.start_session()?;

    // Change this to OpenGL::V2_1 if not working
    let opengl = OpenGL::V3_2;

    // Create a Glutin window
    let mut window: Window = WindowSettings::new("Box Game", [WINDOW_WIDTH, WINDOW_HEIGHT])
        .graphics_api(opengl)
        .exit_on_esc(true)
        .build()
        .unwrap();

    // load a font to render text
    let assets = find_folder::Search::ParentsThenKids(3, 3)
        .for_folder("assets")
        .unwrap();
    let freetype = ft::Library::init().unwrap();
    let font = assets.join("FiraSans-Regular.ttf");

    // Create a new box game
    let mut game = BoxGame::new(font);
    let mut gl = GlGraphics::new(opengl);

    // event settings
    let mut event_settings = EventSettings::new();
    event_settings.set_ups(FPS);
    event_settings.set_max_fps(FPS);
    let mut events = Events::new(event_settings);

    let mut frames_to_skip = 0;

    // event loop
    while let Some(e) = events.next(&mut window) {
        // render
        if let Some(args) = e.render_args() {
            game.render(&mut gl, &freetype, &args);
        }

        // game update
        if let Some(_) = e.update_args() {
            if frames_to_skip > 0 {
                frames_to_skip -= 1;
                println!("Skipping a frame.");
            } else if sess.current_state() == SessionState::Running {
                // tell GGRS it is time to advance the frame and handle the requests
                let local_input = game.local_input();

                match sess.advance_frame(local_handle, &local_input) {
                    Ok(requests) => game.handle_requests(requests),
                    Err(ggrs::GGRSError::PredictionThreshold) => {
                        println!("PredictionThreshold reached, skipping a frame.")
                    }
                    Err(e) => return Err(Box::new(e)),
                }

                // handle GGRS events
                for event in sess.events() {
                    if let GGRSEvent::WaitRecommendation { skip_frames } = event {
                        frames_to_skip += skip_frames
                    }
                    println!("Event: {:?}", event);
                }
            }
        }

        // idle
        if let Some(_args) = e.idle_args() {
            sess.poll_remote_clients();
        }

        // update key state
        if let Some(Button::Keyboard(key)) = e.press_args() {
            match key {
                Key::W => game.wasd_pressed[0] = true,
                Key::A => game.wasd_pressed[1] = true,
                Key::S => game.wasd_pressed[2] = true,
                Key::D => game.wasd_pressed[3] = true,
                _ => (),
            }
        }

        // update key state
        if let Some(Button::Keyboard(key)) = e.release_args() {
            match key {
                Key::W => game.wasd_pressed[0] = false,
                Key::A => game.wasd_pressed[1] = false,
                Key::S => game.wasd_pressed[2] = false,
                Key::D => game.wasd_pressed[3] = false,
                _ => (),
            }
        }
    }

    Ok(())
}
