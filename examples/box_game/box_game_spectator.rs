extern crate freetype as ft;

use ggrs::{GGRSError, GGRSEvent, SessionState};
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{EventLoop, IdleEvent};
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
    assert_eq!(args.len(), 3);

    let port: u16 = args[1].parse()?;
    let host_addr: SocketAddr = args[2].parse()?;

    // create a GGRS session for a spectator
    let mut sess =
        ggrs::start_p2p_spectator_session(NUM_PLAYERS as u32, INPUT_SIZE, port, host_addr)?;

    // start the GGRS session
    sess.start_session()?;

    // Change this to OpenGL::V2_1 if not working
    let opengl = OpenGL::V3_2;

    // Create a Glutin window
    let mut window: Window =
        WindowSettings::new("Box Game Spectator", [WINDOW_WIDTH, WINDOW_HEIGHT])
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
        // render update
        if let Some(args) = e.render_args() {
            game.render(&mut gl, &freetype, &args);
        }

        // game update - tell GGRS it is time to advance the frame and handle the requests
        if let Some(_) = e.update_args() {
            if sess.current_state() == SessionState::Running {
                match sess.advance_frame() {
                    Ok(requests) => game.handle_requests(requests),
                    Err(GGRSError::PredictionThreshold) => {
                        println!("Skipping a frame: Waiting for input from host.");
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
                println!("Event: {:?}", event);
                if let GGRSEvent::Disconnected { .. } = event {
                    println!("Disconnected from host.");
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
