use adler::Adler32;
use bincode;
use ggrs::{GGRSError, P2PSession, SessionState};
use ggrs::{GGRSInterface, GameInput, GameState, PlayerHandle, PlayerType};
use serde::{Deserialize, Serialize};
use std::env;
use std::hash::Hash;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const INPUT_SIZE: usize = std::mem::size_of::<u32>();

struct BoxGameRunner {
    pub game: BoxGame,
    pub sess: P2PSession,
}

impl BoxGameRunner {
    pub fn new(sess: P2PSession) -> Self {
        BoxGameRunner {
            game: BoxGame::new(),
            sess,
        }
    }

    pub fn run(&mut self) -> Result<(), GGRSError> {
        self.sess.start_session()?;

        let mut next = Instant::now();
        loop {
            if Instant::now() >= next {
                next = next + Duration::from_millis(17); // pseudo 60 FPS
                if self.sess.current_state() == SessionState::Running {
                    // do stuff only when the session is ready
                }

                // in any case, get events
                for event in self.sess.events() {
                    println!("Event!: {:?}", event);
                }
            } else {
                // not time for the next frame, let ggrs do some internal work
                self.sess.idle();
            }
        }
    }
}

struct BoxGame {
    pub gs: BoxGameState,
}

impl BoxGame {
    pub fn new() -> Self {
        BoxGame {
            gs: BoxGameState::new(),
        }
    }
}

impl GGRSInterface for BoxGame {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.gs).unwrap();
        let mut adler = Adler32::new();
        self.gs.hash(&mut adler);
        let checksum = adler.checksum();
        GameState {
            frame: self.gs.frame,
            buffer,
            checksum: Some(checksum),
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.gs = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.gs.advance_frame(inputs);
    }
}

#[derive(Hash, Default, Serialize, Deserialize)]
struct BoxGameState {
    pub frame: i32,
    pub state: i32,
}

impl BoxGameState {
    fn new() -> Self {
        Self::default()
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        let p0_inputs: u32 = bincode::deserialize(inputs[0].input()).unwrap();
        let p1_inputs: u32 = bincode::deserialize(inputs[1].input()).unwrap();

        if (p0_inputs + p1_inputs) % 2 == 0 {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // read cmd line arguments very clumsily
    let args: Vec<String> = env::args().collect();
    assert_eq!(args.len(), 4);

    let port: u16 = args[1].parse()?;
    let local_handle: PlayerHandle = args[2].parse()?;
    let remote_handle: PlayerHandle = 1 - local_handle;
    let remote_addr: SocketAddr = args[3].parse()?;

    // create the session with two players
    let mut sess = ggrs::start_p2p_session(2, INPUT_SIZE, port)?;

    // add players
    sess.add_player(PlayerType::Local, local_handle)?;
    sess.add_player(PlayerType::Remote(remote_addr), remote_handle)?;

    // create the BoxGameRunner
    let mut bgr = BoxGameRunner::new(sess);

    //start BoxGameRunner
    println!("Starting the game loop.");
    bgr.run()?;

    Ok(())
}
