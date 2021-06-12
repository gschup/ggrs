use adler::Adler32;
use ggrs::{
    GGRSError, GGRSEvent, GGRSInterface, GameInput, GameState, PlayerHandle, PlayerType,
    SessionState,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::hash::Hash;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const INPUT_SIZE: usize = std::mem::size_of::<u32>();

struct BoxGame {
    pub game_state: BoxGameState, // the game state
}

impl BoxGame {
    pub fn new() -> Self {
        BoxGame {
            game_state: BoxGameState { frame: 0, var: 0 },
        }
    }
}

impl GGRSInterface for BoxGame {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.game_state).unwrap();
        let mut adler = Adler32::new();
        self.game_state.hash(&mut adler);
        let checksum = adler.checksum();
        GameState {
            frame: self.game_state.frame,
            buffer,
            checksum: Some(checksum),
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.game_state = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let p0_inputs: u32 = bincode::deserialize(inputs[0].input()).unwrap();
        let p1_inputs: u32 = bincode::deserialize(inputs[1].input()).unwrap();

        if (p0_inputs + p1_inputs) % 2 == 0 {
            self.game_state.var += 2;
        } else {
            self.game_state.var -= 1;
        }
        self.game_state.frame += 1;
    }
}

#[derive(Hash, Serialize, Deserialize)]
struct BoxGameState {
    pub frame: i32,
    pub var: i32,
}

fn local_input() -> Vec<u8> {
    let input: u32 = 5;
    bincode::serialize(&input).unwrap()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments very clumsily
    let args: Vec<String> = env::args().collect();
    assert_eq!(args.len(), 4);

    let port: u16 = args[1].parse()?;
    let local_handle: PlayerHandle = args[2].parse()?;
    let remote_handle: PlayerHandle = 1 - local_handle;
    let remote_addr: SocketAddr = args[3].parse()?;

    // create a GGRS session with two players
    let mut sess = ggrs::start_p2p_session(2, INPUT_SIZE, port)?;

    // add players
    sess.add_player(PlayerType::Local, local_handle)?;
    sess.add_player(PlayerType::Remote(remote_addr), remote_handle)?;

    // start the GGRS session
    sess.start_session()?;

    // create the game
    let mut game = BoxGame::new();

    let mut next = Instant::now();
    let mut frames_to_skip = 0;
    loop {
        if Instant::now() >= next {
            // almost 60 fps
            next += Duration::from_millis(17);
            // print current state
            println!(
                "State: {:?}, Frame {}",
                sess.current_state(),
                game.game_state.frame
            );

            // do stuff only when the session is ready
            if sess.current_state() != SessionState::Running {
                continue;
            }

            // skip frames, if recommended
            if frames_to_skip > 0 {
                frames_to_skip += 1;
                continue;
            }

            // get local input
            let local_input = local_input();

            // add local input
            match sess.add_local_input(local_handle, &local_input) {
                Ok(()) => {
                    sess.advance_frame(&mut game)?;
                }
                Err(GGRSError::PredictionThreshold) => {
                    println!("PredictionThreshold reached, skipping a frame.");
                }
                Err(e) => {
                    return Err(Box::new(e));
                }
            };
        } else {
            // not time for the next frame, let ggrs do some internal work
            sess.idle();
        }
        // in any case, get and handle ggrs events
        for event in sess.events() {
            if let GGRSEvent::WaitRecommendation { skip_frames } = event {
                frames_to_skip += skip_frames
            }
            println!("Event: {:?}", event);
        }
    }
}
