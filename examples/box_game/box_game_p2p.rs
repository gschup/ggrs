extern crate freetype as ft;

use ggrs::{GGRSEvent, PlayerHandle, PlayerType, SessionState};
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{Button, EventLoop, IdleEvent, Key, PressEvent, ReleaseEvent};
use std::env;
use std::net::SocketAddr;

const FPS: u64 = 60;
const NUM_PLAYERS: usize = 2;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

mod box_game;

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
    let mut game = box_game::BoxGame::new(font);
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
                println!("Skipping a frame: WaitRecommendation");
            } else if sess.current_state() == SessionState::Running {
                // tell GGRS it is time to advance the frame and handle the requests
                let local_input = game.local_input();

                match sess.advance_frame(local_handle, &local_input) {
                    Ok(requests) => game.handle_requests(requests),
                    Err(ggrs::GGRSError::PredictionThreshold) => {
                        println!("Skipping a frame: PredictionThreshold")
                    }
                    Err(e) => return Err(Box::new(e)),
                }
            }
        }

        // idle
        if let Some(_args) = e.idle_args() {
            sess.poll_remote_clients();

            // handle GGRS events
            for event in sess.events() {
                if let GGRSEvent::WaitRecommendation { skip_frames } = event {
                    frames_to_skip += skip_frames
                }
                println!("Event: {:?}", event);
            }
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
