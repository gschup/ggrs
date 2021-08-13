extern crate freetype as ft;
use nalgebra::{vector, Vector2};
use rapier2d::na::ComplexField;
use rapier2d::prelude::*;

use ft::Library;
use ggrs::{
    Frame, GGRSRequest, GameInput, GameState, GameStateCell, PlayerHandle, MAX_PLAYERS, NULL_FRAME,
};
use graphics::{Context, Graphics, ImageSize};
use opengl_graphics::{GlGraphics, Texture, TextureSettings};
use piston::input::RenderArgs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CHECKSUM_PERIOD: i32 = 100;

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const ORANGE: [f32; 4] = [0.78, 0.59, 0.2, 1.0];
const BLUE: [f32; 4] = [0.0, 0.35, 0.78, 1.0];

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;
const SCALE: f64 = 10.;

//const GRAVITY: Vec<f32> = vector![0.0, -9.81];

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

// RapierGame will handle rendering, gamestate, inputs and GGRSRequests
pub struct RapierGame {
    num_players: usize,
    state: RapierState,
    font: PathBuf,
    freetype: Library,
    last_checksum: (Frame, u64),
    periodic_checksum: (Frame, u64),

    // rapier stuff
    physics_pipeline: PhysicsPipeline,
    integration_parameters: IntegrationParameters,
    gravity: Vector2<f32>,
    ccd_solver: CCDSolver,
    physics_hooks: (),
    event_handler: (),
}

impl RapierGame {
    pub fn new(num_players: usize, num_bodies: usize) -> Self {
        // load a font to render text
        let assets = find_folder::Search::ParentsThenKids(3, 3)
            .for_folder("assets")
            .unwrap();
        assert!(num_players <= MAX_PLAYERS as usize);
        Self {
            num_players,
            state: RapierState::new(num_players, num_bodies),
            font: assets.join("FiraSans-Regular.ttf"),
            freetype: ft::Library::init().unwrap(),
            last_checksum: (NULL_FRAME, 0),
            periodic_checksum: (NULL_FRAME, 0),

            physics_pipeline: PhysicsPipeline::new(),
            gravity: vector![0.0, -9.81],
            integration_parameters: IntegrationParameters::default(),
            ccd_solver: CCDSolver::new(),
            physics_hooks: (),
            event_handler: (),
        }
    }

    // for each request, call the appropriate function
    pub fn handle_requests(&mut self, requests: Vec<GGRSRequest>) {
        for request in requests {
            match request {
                GGRSRequest::LoadGameState { cell } => self.load_game_state(cell),
                GGRSRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GGRSRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    // serialize current gamestate, create a checksum
    // creating a checksum here is only relevant for SyncTestSessions
    fn save_game_state(&mut self, cell: GameStateCell, frame: Frame) {
        assert_eq!(self.state.frame, frame);
        let buffer = bincode::serialize(&self.state).unwrap();
        let checksum = fletcher16(&buffer) as u64;

        // remember checksum to render it later
        self.last_checksum = (self.state.frame, checksum);
        if self.state.frame % CHECKSUM_PERIOD == 0 {
            self.periodic_checksum = (self.state.frame, checksum);
        }

        cell.save(GameState::new(frame, Some(buffer), Some(checksum)));
    }

    // deserialize gamestate to load and overwrite current gamestate
    fn load_game_state(&mut self, cell: GameStateCell) {
        let state_to_load = cell.load();
        self.state = bincode::deserialize(&state_to_load.buffer.unwrap()).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        // increase the frame counter
        self.state.frame += 1;

        for i in 0..self.num_players {
            // get input of that player
            let _input: u8 = if inputs[i].frame == NULL_FRAME {
                // disconnected players spin
                4
            } else {
                // otherwise deserialize the input
                bincode::deserialize(inputs[i].input()).unwrap()
            };
        }

        // physics update
        self.physics_pipeline.step(
            &self.gravity,
            &self.integration_parameters,
            &mut self.state.island_manager,
            &mut self.state.broad_phase,
            &mut self.state.narrow_phase,
            &mut self.state.bodies,
            &mut self.state.colliders,
            &mut self.state.joint_set,
            &mut self.ccd_solver,
            &self.physics_hooks,
            &self.event_handler,
        );
    }

    // renders the game to the window
    pub fn render(&mut self, gl: &mut GlGraphics, args: &RenderArgs) {
        use graphics::*;

        // preparation for last checksum rendering
        let mut face = self.freetype.new_face(&self.font, 0).unwrap();
        face.set_pixel_sizes(0, 40).unwrap();
        let checksum_string = format!(
            "Frame {}: Checksum {}",
            self.last_checksum.0, self.last_checksum.1
        );
        let checksum_glyphs = glyphs(&mut face, &checksum_string);
        // preparation for periodic checksum rendering
        let periodic_string = format!(
            "Frame {}: Checksum {}",
            self.periodic_checksum.0, self.periodic_checksum.1
        );
        let periodic_glyphs = glyphs(&mut face, &periodic_string);

        // start drawing
        gl.draw(args.viewport(), |c, gl| {
            // clear the screen
            clear(BLACK, gl);

            // render checksums
            render_text(&checksum_glyphs, &c.trans(0.0, 40.0), gl);
            render_text(&periodic_glyphs, &c.trans(0.0, 80.0), gl);

            for sphere_handle in &self.state.sphere_handles {
                let sphere_body = &self.state.bodies[*sphere_handle];
                let rect = rectangle::square(0.0, 0.0, SCALE);
                let transform = c
                    .transform
                    .trans(
                        sphere_body.translation().x as f64 * SCALE,
                        WINDOW_HEIGHT as f64 - sphere_body.translation().y as f64 * SCALE,
                    )
                    .trans(WINDOW_WIDTH as f64 / 2., -30.)
                    .rot_rad(sphere_body.rotation().angle() as f64)
                    .trans(-SCALE / 2., -SCALE / 2.);
                ellipse(BLUE, rect, transform, gl);
            }

            for cube_handle in &self.state.cube_handles {
                let cube_body = &self.state.bodies[*cube_handle];
                let rect = rectangle::square(0.0, 0.0, SCALE);
                let transform = c
                    .transform
                    .trans(
                        cube_body.translation().x as f64 * SCALE,
                        WINDOW_HEIGHT as f64 - cube_body.translation().y as f64 * SCALE,
                    )
                    .trans(WINDOW_WIDTH as f64 / 2., -30.)
                    .rot_rad(cube_body.rotation().angle() as f64)
                    .trans(-SCALE / 2., -SCALE / 2.);
                rectangle(ORANGE, rect, transform, gl);
            }
        });
    }

    #[allow(dead_code)]
    // creates a compact representation of currently pressed keys and serializes it
    pub fn local_input(&self, _handle: PlayerHandle) -> Vec<u8> {
        vec![0u8]
    }
}

// BoxGameState holds all relevant information about the game state
#[derive(Serialize, Deserialize)]
struct RapierState {
    pub frame: i32,
    pub num_players: usize,

    // rapier stuff
    bodies: RigidBodySet,
    colliders: ColliderSet,
    joint_set: JointSet,
    broad_phase: BroadPhase,
    narrow_phase: NarrowPhase,
    island_manager: IslandManager,

    cube_handles: Vec<RigidBodyHandle>,
    sphere_handles: Vec<RigidBodyHandle>,
}

impl RapierState {
    pub fn new(num_players: usize, num_bodies: usize) -> Self {
        /*
         * World
         */
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();

        /*
         * Ground
         */
        let ground_size = 50.0;
        let nsubdivs = 2000;
        let step_size = ground_size / (nsubdivs as f32);
        let mut points = Vec::new();

        points.push(point![-ground_size / 2.0, 40.0]);
        for i in 1..nsubdivs - 1 {
            let x = -ground_size / 2.0 + i as f32 * step_size;
            let y = ComplexField::cos(i as f32 * step_size) * 2.0;
            points.push(point![x, y]);
        }
        points.push(point![ground_size / 2.0, 40.0]);

        let rigid_body = RigidBodyBuilder::new_static().build();
        let handle = bodies.insert(rigid_body);
        let collider = ColliderBuilder::polyline(points, None).build();
        colliders.insert_with_parent(collider, handle, &mut bodies);

        /*
         * Create the bodies
         */
        let rad = 0.5;

        let mut cube_handles = Vec::with_capacity(num_bodies / 2);
        let mut sphere_handles = Vec::with_capacity(num_bodies / 2);

        let shift = rad * 2.0;
        let centerx = shift * (num_bodies / 2) as f32;
        let centery = shift / 2.0 + 20.0;

        for i in 0..num_bodies {
            for j in 0usize..num_bodies {
                let x = i as f32 * shift - centerx;
                let y = j as f32 * shift + centery + 3.0;

                // Build the rigid body.
                let rigid_body = RigidBodyBuilder::new_dynamic()
                    .translation(vector![x, y])
                    .build();
                let handle = bodies.insert(rigid_body);

                if j % 2 == 0 {
                    let collider = ColliderBuilder::cuboid(rad, rad).build();
                    colliders.insert_with_parent(collider, handle, &mut bodies);
                    cube_handles.push(handle);
                } else {
                    let collider = ColliderBuilder::ball(rad).build();
                    colliders.insert_with_parent(collider, handle, &mut bodies);
                    sphere_handles.push(handle);
                }
            }
        }

        Self {
            frame: 0,
            num_players,

            bodies,
            colliders,
            joint_set: JointSet::new(),
            broad_phase: BroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            island_manager: IslandManager::new(),

            cube_handles,
            sphere_handles,
        }
    }
}
