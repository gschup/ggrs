use std::net::SocketAddr;

use bytemuck::{Pod, Zeroable};
use ggrs::{Config, Frame, GGRSRequest, GameStateCell, InputStatus, PlayerHandle, NULL_FRAME};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

const FPS: u64 = 60;
const CHECKSUM_PERIOD: i32 = 100;

const SHIP_HEIGHT: f32 = 50.;
const SHIP_BASE: f32 = 40.;
const WINDOW_HEIGHT: f32 = 800.0;
const WINDOW_WIDTH: f32 = 600.0;

const INPUT_UP: u8 = 1 << 0;
const INPUT_DOWN: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;

const MOVEMENT_SPEED: f32 = 15.0 / FPS as f32;
const ROTATION_SPEED: f32 = 2.5 / FPS as f32;
const MAX_SPEED: f32 = 7.0;
const FRICTION: f32 = 0.98;

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct Input {
    pub inp: u8,
}

/// `GGRSConfig` holds all type parameters for GGRS Sessions
#[derive(Debug)]
pub struct GGRSConfig;
impl Config for GGRSConfig {
    type Input = Input;
    type State = State;
    type Address = SocketAddr;
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

// BoxGame will handle rendering, gamestate, inputs and GGRSRequests
pub struct Game {
    num_players: usize,
    game_state: State,
    local_handles: Vec<PlayerHandle>,
    last_checksum: (Frame, u64),
    periodic_checksum: (Frame, u64),
}

impl Game {
    pub fn new(num_players: usize) -> Self {
        assert!(num_players <= 4);
        Self {
            num_players,
            game_state: State::new(num_players),
            local_handles: Vec::new(),
            last_checksum: (NULL_FRAME, 0),
            periodic_checksum: (NULL_FRAME, 0),
        }
    }

    // for each request, call the appropriate function
    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest<GGRSConfig>>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    // save current gamestate, create a checksum
    // creating a checksum here is only relevant for SyncTestSessions
    fn save_game_state(&mut self, cell: GameStateCell<State>, frame: Frame) {
        assert_eq!(self.game_state.frame, frame);
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let checksum = fletcher16(&buffer) as u128;
        cell.save(frame, Some(self.game_state.clone()), Some(checksum));
    }

    // load gamestate and overwrite
    fn load_game_state(&mut self, cell: GameStateCell<State>) {
        self.game_state = cell.load().expect("No data found.");
    }

    fn advance_frame(&mut self, inputs: Vec<(Input, InputStatus)>) {
        // advance the game state
        self.game_state.advance(inputs);

        // remember checksum to render it later
        // it is very inefficient to serialize the gamestate here just for the checksum
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let checksum = fletcher16(&buffer) as u64;
        self.last_checksum = (self.game_state.frame, checksum);
        if self.game_state.frame % CHECKSUM_PERIOD == 0 {
            self.periodic_checksum = (self.game_state.frame, checksum);
        }
    }

    // renders the game to the window
    pub fn render(&self) {
        clear_background(BLACK);

        // render players
        for i in 0..self.num_players {
            let color = match i {
                0 => GOLD,
                1 => BLUE,
                2 => GREEN,
                3 => RED,
                _ => WHITE,
            };
            let (x, y) = self.game_state.positions[i];
            let rotation = self.game_state.rotations[i] + std::f32::consts::PI / 2.0;
            let v1 = Vec2::new(
                x + rotation.sin() * SHIP_HEIGHT / 2.,
                y - rotation.cos() * SHIP_HEIGHT / 2.,
            );
            let v2 = Vec2::new(
                x - rotation.cos() * SHIP_BASE / 2. - rotation.sin() * SHIP_HEIGHT / 2.,
                y - rotation.sin() * SHIP_BASE / 2. + rotation.cos() * SHIP_HEIGHT / 2.,
            );
            let v3 = Vec2::new(
                x + rotation.cos() * SHIP_BASE / 2. - rotation.sin() * SHIP_HEIGHT / 2.,
                y + rotation.sin() * SHIP_BASE / 2. + rotation.cos() * SHIP_HEIGHT / 2.,
            );
            draw_triangle(v1, v2, v3, color);
        }

        // render checksums
        let last_checksum_str = format!(
            "Frame {}: Checksum {}",
            self.last_checksum.0, self.last_checksum.1
        );
        let periodic_checksum_str = format!(
            "Frame {}: Checksum {}",
            self.periodic_checksum.0, self.periodic_checksum.1
        );
        draw_text(&last_checksum_str, 20.0, 20.0, 30.0, WHITE);
        draw_text(&periodic_checksum_str, 20.0, 40.0, 30.0, WHITE);
    }

    #[allow(dead_code)]
    pub fn register_local_handles(&mut self, handles: Vec<PlayerHandle>) {
        self.local_handles = handles
    }

    #[allow(dead_code)]
    // creates a compact representation of currently pressed keys and serializes it
    pub fn local_input(&self, handle: PlayerHandle) -> Input {
        let mut inp: u8 = 0;

        if handle == self.local_handles[0] {
            // first local player with WASD
            if is_key_down(KeyCode::W) {
                inp |= INPUT_UP;
            }
            if is_key_down(KeyCode::A) {
                inp |= INPUT_LEFT;
            }
            if is_key_down(KeyCode::S) {
                inp |= INPUT_DOWN;
            }
            if is_key_down(KeyCode::D) {
                inp |= INPUT_RIGHT;
            }
        } else {
            // all other local players with arrow keys
            if is_key_down(KeyCode::Up) {
                inp |= INPUT_UP;
            }
            if is_key_down(KeyCode::Left) {
                inp |= INPUT_LEFT;
            }
            if is_key_down(KeyCode::Down) {
                inp |= INPUT_DOWN;
            }
            if is_key_down(KeyCode::Right) {
                inp |= INPUT_RIGHT;
            }
        }

        Input { inp }
    }

    #[allow(dead_code)]
    pub const fn current_frame(&self) -> i32 {
        self.game_state.frame
    }
}

// BoxGameState holds all relevant information about the game state
#[derive(Clone, Serialize, Deserialize)]
pub struct State {
    pub frame: i32,
    pub num_players: usize,
    pub positions: Vec<(f32, f32)>,
    pub velocities: Vec<(f32, f32)>,
    pub rotations: Vec<f32>,
}

impl State {
    pub fn new(num_players: usize) -> Self {
        let mut positions = Vec::new();
        let mut velocities = Vec::new();
        let mut rotations = Vec::new();

        let r = WINDOW_WIDTH as f32 / 4.0;

        for i in 0..num_players as i32 {
            let rot = i as f32 / num_players as f32 * 2.0 * std::f32::consts::PI;
            let x = WINDOW_WIDTH as f32 / 2.0 + r * rot.cos();
            let y = WINDOW_HEIGHT as f32 / 2.0 + r * rot.sin();
            positions.push((x as f32, y as f32));
            velocities.push((0.0, 0.0));
            rotations.push((rot + std::f32::consts::PI) % (2.0 * std::f32::consts::PI));
        }

        Self {
            frame: 0,
            num_players,
            positions,
            velocities,
            rotations,
        }
    }

    pub fn advance(&mut self, inputs: Vec<(Input, InputStatus)>) {
        // increase the frame counter
        self.frame += 1;

        for i in 0..self.num_players {
            // get input of that player
            let input = match inputs[i].1 {
                InputStatus::Confirmed => inputs[i].0.inp,
                InputStatus::Predicted => inputs[i].0.inp,
                InputStatus::Disconnected => 4, // disconnected players spin
            };

            // old values
            let (old_x, old_y) = self.positions[i];
            let (old_vel_x, old_vel_y) = self.velocities[i];
            let mut rot = self.rotations[i];

            // slow down
            let mut vel_x = old_vel_x * FRICTION;
            let mut vel_y = old_vel_y * FRICTION;

            // thrust
            if input & INPUT_UP != 0 && input & INPUT_DOWN == 0 {
                vel_x += MOVEMENT_SPEED * rot.cos();
                vel_y += MOVEMENT_SPEED * rot.sin();
            }
            // break
            if input & INPUT_UP == 0 && input & INPUT_DOWN != 0 {
                vel_x -= MOVEMENT_SPEED * rot.cos();
                vel_y -= MOVEMENT_SPEED * rot.sin();
            }
            // turn left
            if input & INPUT_LEFT != 0 && input & INPUT_RIGHT == 0 {
                rot = (rot - ROTATION_SPEED).rem_euclid(2.0 * std::f32::consts::PI);
            }
            // turn right
            if input & INPUT_LEFT == 0 && input & INPUT_RIGHT != 0 {
                rot = (rot + ROTATION_SPEED).rem_euclid(2.0 * std::f32::consts::PI);
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

            // constrain players to canvas borders
            x = x.max(0.0);
            x = x.min(WINDOW_WIDTH);
            y = y.max(0.0);
            y = y.min(WINDOW_HEIGHT);

            // update all state
            self.positions[i] = (x, y);
            self.velocities[i] = (vel_x, vel_y);
            self.rotations[i] = rot;
        }
    }
}
