extern crate freetype as ft;

use ggrs::SyncTestSession;
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{Button, EventLoop, Key, PressEvent, ReleaseEvent};
use structopt::StructOpt;

const FPS: u64 = 60;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

mod box_game;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, long)]
    num_players: usize,
    #[structopt(short, long)]
    check_distance: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();

    // create a GGRS session
    let mut sess = SyncTestSession::new(opt.num_players as u32, INPUT_SIZE, opt.check_distance)?;

    // set input delay for any player you want
    for i in 0..opt.num_players {
        sess.set_frame_delay(2, i)?;
    }

    // Change this to OpenGL::V2_1 if not working
    let opengl = OpenGL::V3_2;

    // Create a Glutin window
    let mut window: Window =
        WindowSettings::new("Box Game Synctest", [WINDOW_WIDTH, WINDOW_HEIGHT])
            .graphics_api(opengl)
            .exit_on_esc(true)
            .build()
            .unwrap();

    // Create a new box game
    let mut game = box_game::BoxGame::new(opt.num_players);
    let mut gl = GlGraphics::new(opengl);

    // event settings
    let mut event_settings = EventSettings::new();
    event_settings.set_ups(FPS);
    event_settings.set_max_fps(FPS);
    let mut events = Events::new(event_settings);

    // event loop
    while let Some(e) = events.next(&mut window) {
        // render update
        if let Some(args) = e.render_args() {
            game.render(&mut gl, &args);
        }

        // game update
        if let Some(_) = e.update_args() {
            // create inputs for all players
            let mut all_inputs = Vec::new();
            for i in 0..opt.num_players {
                all_inputs.push(game.local_input(i));
            }
            // tell GGRS it is time to advance the frame and handle the requests
            let requests = sess.advance_frame(&all_inputs)?;
            game.handle_requests(requests);
        }

        // key state update
        if let Some(Button::Keyboard(key)) = e.press_args() {
            match key {
                Key::W => game.key_states[0] = true,
                Key::A => game.key_states[1] = true,
                Key::S => game.key_states[2] = true,
                Key::D => game.key_states[3] = true,
                Key::Up => game.key_states[4] = true,
                Key::Left => game.key_states[5] = true,
                Key::Down => game.key_states[6] = true,
                Key::Right => game.key_states[7] = true,
                _ => (),
            }
        }

        // key state update
        if let Some(Button::Keyboard(key)) = e.release_args() {
            match key {
                Key::W => game.key_states[0] = false,
                Key::A => game.key_states[1] = false,
                Key::S => game.key_states[2] = false,
                Key::D => game.key_states[3] = false,
                Key::Up => game.key_states[4] = false,
                Key::Left => game.key_states[5] = false,
                Key::Down => game.key_states[6] = false,
                Key::Right => game.key_states[7] = false,
                _ => (),
            }
        }
    }

    Ok(())
}
