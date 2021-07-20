extern crate freetype as ft;

use ft::Library;
use ggrs::{Frame, GGRSRequest, GameInput, GameState, GameStateCell, NULL_FRAME};
use graphics::{Context, Graphics, ImageSize};
use opengl_graphics::{GlGraphics, Texture, TextureSettings};
use piston::input::RenderArgs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FPS: u64 = 60;
const NUM_PLAYERS: usize = 2;
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

const MOVEMENT_SPEED: f64 = 15.0 / FPS as f64;
const ROTATION_SPEED: f64 = 2.5 / FPS as f64;
const MAX_SPEED: f64 = 7.0;
const FRICTION: f64 = 0.98;

/// Computes the fletcher16 checksum, copied from wikipedia: <https://en.wikipedia.org/wiki/Fletcher%27s_checksum>
fn fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;

    for index in 0..data.len() {
        sum1 = (sum1 + data[index] as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }

    (sum2 << 8) | sum1
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
    pub key_states: [bool; 4],
    font: PathBuf,
    last_checksum: (Frame, u64),
    periodic_checksum: (Frame, u64),
}

impl BoxGame {
    pub fn new(font: PathBuf) -> Self {
        Self {
            game_state: BoxGameState::new(),
            key_states: [false; 4],
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
                input = 4; // disconnected players spin
            } else {
                input = bincode::deserialize(inputs[i].input()).unwrap();
            }

            // old values
            let (old_x, old_y) = self.game_state.positions[i];
            let (old_vel_x, old_vel_y) = self.game_state.velocities[i];
            let mut rot = self.game_state.rotations[i];

            // slow down
            let mut vel_x = old_vel_x * FRICTION;
            let mut vel_y = old_vel_y * FRICTION;

            // thrust
            if input & INPUT_UP != 0 && input & INPUT_DOWN == 0 {
                vel_x += MOVEMENT_SPEED * rot.cos();
                vel_y += MOVEMENT_SPEED * rot.sin();
            }
            //break
            if input & INPUT_UP == 0 && input & INPUT_DOWN != 0 {
                vel_x -= MOVEMENT_SPEED * rot.cos();
                vel_y -= MOVEMENT_SPEED * rot.sin();
            }
            // turn left
            if input & INPUT_LEFT != 0 && input & INPUT_RIGHT == 0 {
                rot = (rot - ROTATION_SPEED).rem_euclid(2.0 * std::f64::consts::PI);
            }
            // turn right
            if input & INPUT_LEFT == 0 && input & INPUT_RIGHT != 0 {
                rot = (rot + ROTATION_SPEED).rem_euclid(2.0 * std::f64::consts::PI);
            }

            // limit speed
            let magnitude = (vel_x * vel_x + vel_y * vel_y).sqrt();
            if magnitude > MAX_SPEED {
                vel_x = (vel_x * MAX_SPEED) / magnitude;
                vel_y = (vel_y * MAX_SPEED) / magnitude;
            }

            // compute new position
            let mut x = old_x + vel_x;
            let mut y = old_y + vel_y;

            //constrain boxes to canvas borders
            x = x.max(0.0);
            x = x.min(WINDOW_WIDTH as f64);
            y = y.max(0.0);
            y = y.min(WINDOW_HEIGHT as f64);

            self.game_state.positions[i] = (x, y);
            self.game_state.velocities[i] = (vel_x, vel_y);
            self.game_state.rotations[i] = rot;
        }

        // TODO: inefficient to serialize the gamestate here just for the checksum
        // remember checksum to render it later
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let checksum = fletcher16(&buffer) as u64;
        self.last_checksum = (self.game_state.frame, checksum);
        if self.game_state.frame % CHECKSUM_PERIOD == 0 {
            self.periodic_checksum = (self.game_state.frame, checksum);
        }
    }

    pub fn render(&mut self, gl: &mut GlGraphics, freetype: &Library, args: &RenderArgs) {
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
                let (x, y) = self.game_state.positions[i];
                let rotation = self.game_state.rotations[i];

                let transform = c
                    .transform
                    .trans(x, y)
                    .rot_rad(rotation)
                    .trans(-PLAYER_SIZE / 2.0, -PLAYER_SIZE / 2.0);
                rectangle(PLAYER_COLORS[i], square, transform, gl);
            }
        });
    }

    #[allow(dead_code)]
    pub fn local_input(&self) -> Vec<u8> {
        // Create a set of pressed Keys.
        let mut input: u8 = 0;

        // ugly, but it works...
        if self.key_states[0] {
            input |= INPUT_UP;
        }
        if self.key_states[1] {
            input |= INPUT_LEFT;
        }
        if self.key_states[2] {
            input |= INPUT_DOWN;
        }
        if self.key_states[3] {
            input |= INPUT_RIGHT;
        }

        bincode::serialize(&input).unwrap()
    }
}

// BoxGameState holds all relevant information about the game state
#[derive(Serialize, Deserialize)]
struct BoxGameState {
    pub frame: i32,
    pub positions: Vec<(f64, f64)>,
    pub velocities: Vec<(f64, f64)>,
    pub rotations: Vec<f64>,
}

impl BoxGameState {
    pub fn new() -> Self {
        let mut positions = Vec::new();
        let mut velocities = Vec::new();
        let mut rotations = Vec::new();
        for i in 0..NUM_PLAYERS as i32 {
            let x = WINDOW_WIDTH as i32 / 2 + (2 * i - 1) * (WINDOW_WIDTH as i32 / 4);
            let y = WINDOW_HEIGHT as i32 / 2;
            positions.push((x as f64, y as f64));
            velocities.push((0.0, 0.0));
            rotations.push(0.0);
        }

        Self {
            frame: 0,
            positions,
            velocities,
            rotations,
        }
    }
}
