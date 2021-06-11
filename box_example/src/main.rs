use adler::Adler32;
use bincode;
use ggrs::{
    GGRSError, GGRSEvent, GGRSInterface, GameInput, GameState, P2PSession, PlayerHandle,
    PlayerType, SessionState,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::hash::Hash;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const INPUT_SIZE: usize = std::mem::size_of::<u32>();

struct BoxGameRunner {
    pub game: BoxGame,
    pub sess: P2PSession,
    local_handle: PlayerHandle,
}

impl BoxGameRunner {
    pub fn new(sess: P2PSession) -> Self {
        BoxGameRunner {
            game: BoxGame::new(),
            sess,
            local_handle: 0,
        }
    }

    pub fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        match self.sess.add_player(player_type, player_handle) {
            Ok(_) => {
                if player_type == PlayerType::Local {
                    self.local_handle = player_handle;
                }
                Ok(())
            }
            Err(e) => return Err(e),
        }
    }

    pub fn run(&mut self) -> Result<(), GGRSError> {
        self.sess.start_session()?;

        let mut next = Instant::now();
        let mut frames_to_skip = 0;
        loop {
            if Instant::now() >= next {
                // almost 60 fps
                next = next + Duration::from_millis(17);
                // print current state
                println!(
                    "State: {:?}, Frame {}",
                    self.sess.current_state(),
                    self.game.state.frame
                );

                // do stuff only when the session is ready
                if self.sess.current_state() != SessionState::Running {
                    continue;
                }

                // skip frames, if recommended
                if frames_to_skip > 0 {
                    frames_to_skip += 1;
                    continue;
                }

                // get local input
                let local_input = self.local_input();

                // add local input
                match self.sess.add_local_input(self.local_handle, &local_input) {
                    Ok(()) => {
                        self.sess.advance_frame(&mut self.game)?;
                    }
                    Err(GGRSError::PredictionThreshold) => {
                        println!("PredictionThreshold reached, skipping a frame.");
                    }
                    Err(e) => {
                        return Err(e);
                    }
                };
            } else {
                // not time for the next frame, let ggrs do some internal work
                self.sess.idle();
            }
            // in any case, get and handle events
            for event in self.sess.events() {
                match event {
                    GGRSEvent::WaitRecommendation { skip_frames } => frames_to_skip += skip_frames,
                    _ => (),
                }
                println!("Event: {:?}", event);
            }
        }
    }

    fn local_input(&self) -> Vec<u8> {
        let input: u32 = 5;
        bincode::serialize(&input).unwrap()
    }
}

struct BoxGame {
    pub state: BoxGameState,
}

impl BoxGame {
    pub fn new() -> Self {
        BoxGame {
            state: BoxGameState::new(),
        }
    }
}

impl GGRSInterface for BoxGame {
    fn save_game_state(&self) -> GameState {
        let buffer = bincode::serialize(&self.state).unwrap();
        let mut adler = Adler32::new();
        self.state.hash(&mut adler);
        let checksum = adler.checksum();
        GameState {
            frame: self.state.frame,
            buffer,
            checksum: Some(checksum),
        }
    }

    fn load_game_state(&mut self, state: &GameState) {
        self.state = bincode::deserialize(&state.buffer).unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<GameInput>) {
        self.state.advance_frame(inputs);
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
    let sess = ggrs::start_p2p_session(2, INPUT_SIZE, port)?;

    // create the BoxGameRunner
    let mut bgr = BoxGameRunner::new(sess);

    // add players
    bgr.add_player(PlayerType::Local, local_handle)?;
    bgr.add_player(PlayerType::Remote(remote_addr), remote_handle)?;

    //start BoxGameRunner
    println!("Starting the game loop.");
    bgr.run()?;

    Ok(())
}
