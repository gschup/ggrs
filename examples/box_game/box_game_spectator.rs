extern crate freetype as ft;

use ggrs::{GGRSError, GGRSEvent, SessionState};
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{EventLoop, IdleEvent};
use std::net::SocketAddr;
use structopt::StructOpt;

const FPS: u64 = 60;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

mod box_game;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, long)]
    local_port: u16,
    #[structopt(short, long)]
    num_players: usize,
    #[structopt(short, long)]
    host: SocketAddr,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();

    // create a GGRS session for a spectator
    let mut sess = ggrs::new_p2p_spectator_session(
        opt.num_players as u32,
        INPUT_SIZE,
        opt.local_port,
        opt.host,
    )?;

    // change catch-up parameters, if desired
    sess.set_max_frames_behind(5)?; // when the spectator is more than this amount of frames behind, it will catch up
    sess.set_catchup_speed(2)?; // set this to 1 if you don't want any catch-ups

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

        // game update - tell GGRS it is time to advance the frame and handle the requests
        if let Some(_) = e.update_args() {
            if sess.current_state() == SessionState::Running {
                match sess.advance_frame() {
                    Ok(requests) => game.handle_requests(requests),
                    Err(GGRSError::PredictionThreshold) => {
                        println!(
                            "Frame {} skipped: Waiting for input from host.",
                            game.current_frame()
                        );
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
