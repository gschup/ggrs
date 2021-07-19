extern crate freetype as ft;

use ggrs::{PlayerType, SessionState};
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{Button, EventLoop, IdleEvent, Key, PressEvent, ReleaseEvent};

const FPS: u64 = 60;
const NUM_PLAYERS: usize = 2;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();
const CHECK_DISTANCE: u32 = 7;

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

mod box_game;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a GGRS session with two players
    let mut sess = ggrs::start_synctest_session(NUM_PLAYERS as u32, INPUT_SIZE, CHECK_DISTANCE)?;

    // add player - this is a synctest, we skip the second player
    sess.add_player(PlayerType::Local, 0)?;

    // set input delay for the local player
    sess.set_frame_delay(2, 0)?;

    // start the GGRS session
    sess.start_session()?;

    // Change this to OpenGL::V2_1 if not working
    let opengl = OpenGL::V3_2;

    // Create a Glutin window
    let mut window: Window =
        WindowSettings::new("Box Game Synctest", [WINDOW_WIDTH, WINDOW_HEIGHT])
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
    let mut game = box_game::BoxGame::new(font);
    let mut gl = GlGraphics::new(opengl);

    // event settings
    let mut event_settings = EventSettings::new();
    event_settings.set_ups(FPS);
    event_settings.set_max_fps(FPS);
    let mut events = Events::new(event_settings);

    // event loop
    while let Some(e) = events.next(&mut window) {
        // render
        if let Some(args) = e.render_args() {
            game.render(&mut gl, &freetype, &args);
        }

        // game update
        if let Some(_) = e.update_args() {
            // do stuff only when the session is ready
            if sess.current_state() == SessionState::Running {
                // tell GGRS it is time to advance the frame and handle the requests
                let local_input = game.local_input();
                let requests = sess.advance_frame(0, &local_input)?;

                // handle requests
                game.handle_requests(requests);
            }
        }

        // idle
        if let Some(_args) = e.idle_args() {
            // poll remote endpoints, but not in the synctest
        }

        // update key state
        if let Some(Button::Keyboard(key)) = e.press_args() {
            match key {
                Key::W => game.key_states[0] = true,
                Key::A => game.key_states[1] = true,
                Key::S => game.key_states[2] = true,
                Key::D => game.key_states[3] = true,
                _ => (),
            }
        }

        // update key state
        if let Some(Button::Keyboard(key)) = e.release_args() {
            match key {
                Key::W => game.key_states[0] = false,
                Key::A => game.key_states[1] = false,
                Key::S => game.key_states[2] = false,
                Key::D => game.key_states[3] = false,
                _ => (),
            }
        }
    }

    Ok(())
}
