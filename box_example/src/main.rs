use adler::Adler32;
use bincode;
use ggrs::{GGRSInterface, GameInput, GameState, PlayerHandle, PlayerType};
use serde::{Deserialize, Serialize};
use std::env;
use std::hash::Hash;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

const INPUT_SIZE: usize = std::mem::size_of::<u32>();

pub struct BoxGame {
    pub gs: BoxGameState,
}

impl BoxGame {
    pub fn new() -> BoxGame {
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
pub struct BoxGameState {
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

    //start session
    sess.start_session()?;

    let mut count = 0;
    loop {
        sess.idle();
        for event in sess.events() {
            println!("Event!: {:?}", event);
        }
        thread::sleep(Duration::from_millis(10));
        count += 1;
        if count % 100 == 0 {
            println!("State: {:?}", sess.current_state());
        }
    }
}
