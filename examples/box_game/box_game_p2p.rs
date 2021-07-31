extern crate freetype as ft;

use ggrs::{GGRSEvent, PlayerType, SessionState};
use glutin_window::GlutinWindow as Window;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::{EventSettings, Events};
use piston::input::{RenderEvent, UpdateEvent};
use piston::window::WindowSettings;
use piston::{Button, EventLoop, IdleEvent, Key, PressEvent, ReleaseEvent};
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
    players: Vec<String>,
    #[structopt(short, long)]
    spectators: Vec<SocketAddr>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments
    let opt = Opt::from_args();
    let mut local_handle = 0;
    let num_players = opt.players.len();
    assert!(num_players > 0);

    // create a GGRS session with two players
    let mut sess = ggrs::start_p2p_session(num_players as u32, INPUT_SIZE, opt.local_port)?;

    // add players
    for (i, player_addr) in opt.players.iter().enumerate() {
        // local player
        if player_addr == "localhost" {
            sess.add_player(PlayerType::Local, i)?;
            local_handle = i;
        } else {
            // remote players
            let remote_addr: SocketAddr = player_addr.parse()?;
            sess.add_player(PlayerType::Remote(remote_addr), i)?;
        }
    }

    // optionally, add spectators
    for (i, spec_addr) in opt.spectators.iter().enumerate() {
        sess.add_player(PlayerType::Spectator(*spec_addr), num_players + i)?;
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
    let mut game = box_game::BoxGame::new(num_players, font);
    let mut gl = GlGraphics::new(opengl);

    // event settings
    let mut event_settings = EventSettings::new();
    event_settings.set_ups(FPS);
    event_settings.set_max_fps(FPS);
    let mut events = Events::new(event_settings);

    let mut frames_to_skip = 0;

    // event loop
    while let Some(e) = events.next(&mut window) {
        // render update
        if let Some(args) = e.render_args() {
            game.render(&mut gl, &freetype, &args);
        }

        // game update
        if let Some(_) = e.update_args() {
            //skip frames if recommended
            if frames_to_skip > 0 {
                frames_to_skip -= 1;
                println!("Skipping a frame: WaitRecommendation");
                continue;
            }

            // if the session is running, tell GGRS it is time to advance the frame and handle the requests
            if sess.current_state() == SessionState::Running {
                // always get WASD inputs
                let local_input = game.local_input(0);

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

        // key state update
        if let Some(Button::Keyboard(key)) = e.press_args() {
            match key {
                Key::W => game.key_states[0] = true,
                Key::A => game.key_states[1] = true,
                Key::S => game.key_states[2] = true,
                Key::D => game.key_states[3] = true,
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
                _ => (),
            }
        }
    }

    Ok(())
}
