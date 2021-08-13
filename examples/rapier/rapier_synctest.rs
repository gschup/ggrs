extern crate freetype as ft;

use std::time::Instant;

use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::EventLoop;
use structopt::StructOpt;

const FPS: u64 = 60;
const INPUT_SIZE: usize = std::mem::size_of::<u8>();

const WINDOW_HEIGHT: u32 = 800;
const WINDOW_WIDTH: u32 = 600;

mod rapier_game;

#[derive(StructOpt)]
struct Opt {
    #[structopt(short, long)]
    num_bodies: usize,
    #[structopt(short, long)]
    check_distance: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let num_players = 1;
    // read cmd line arguments
    let opt = Opt::from_args();

    // create a GGRS session
    let mut sess =
        ggrs::start_synctest_session(num_players as u32, INPUT_SIZE, opt.check_distance)?;

    // set input delay for any player you want
    for i in 0..num_players {
        sess.set_frame_delay(2, i)?;
    }

    // Change this to OpenGL::V2_1 if not working
    let opengl = OpenGL::V3_2;

    // Create a Glutin window
    let mut window: Window = WindowSettings::new("Rapier Synctest", [WINDOW_WIDTH, WINDOW_HEIGHT])
        .graphics_api(opengl)
        .exit_on_esc(true)
        .build()
        .unwrap();

    // Create a new box game
    let mut game = rapier_game::RapierGame::new(num_players, opt.num_bodies);
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
            let now = Instant::now();
            // create inputs for all players
            let mut all_inputs = Vec::new();
            for i in 0..num_players {
                all_inputs.push(game.local_input(i));
            }
            // tell GGRS it is time to advance the frame and handle the requests
            let requests = sess.advance_frame(&all_inputs)?;
            game.handle_requests(requests);
            if now.elapsed().as_micros() > 1000000 / FPS as u128 {
                println!(
                    "Update took too long: {} microseconds",
                    now.elapsed().as_micros()
                );
            }
        }
    }

    Ok(())
}
