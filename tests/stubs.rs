use rand::{prelude::ThreadRng, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::thread;
use std::time::{Duration, Instant};

use ggrs::{
    Config, Frame, GameStateCell, GgrsError, GgrsRequest, InputStatus, P2PSession, PlayerType,
    PredictRepeatLast, SessionBuilder, SessionState, SpectatorSession, UdpNonBlockingSocket,
};

fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub struct GameStub {
    pub gs: StateStub,
}

#[repr(C)]
#[derive(Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StubInput {
    pub inp: u32,
}

pub struct StubConfig;

impl Config for StubConfig {
    type Input = StubInput;
    type InputPredictor = PredictRepeatLast;
    type State = StateStub;
    type Address = SocketAddr;
}

impl Default for GameStub {
    fn default() -> Self {
        Self::new()
    }
}

impl GameStub {
    #[allow(dead_code)]
    pub fn new() -> GameStub {
        GameStub {
            gs: StateStub { frame: 0, state: 0 },
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GgrsRequest<StubConfig>>) {
        for request in requests {
            match request {
                GgrsRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GgrsRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GgrsRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStub>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let checksum = calculate_hash(&self.gs);
        cell.save(frame, Some(self.gs), Some(checksum as u128));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStub>) {
        self.gs = cell.load().unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        self.gs.advance_frame(inputs);
    }
}

pub struct RandomChecksumGameStub {
    pub gs: StateStub,
    rng: ThreadRng,
}

impl Default for RandomChecksumGameStub {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomChecksumGameStub {
    #[allow(dead_code)]
    pub fn new() -> RandomChecksumGameStub {
        RandomChecksumGameStub {
            gs: StateStub { frame: 0, state: 0 },
            rng: thread_rng(),
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GgrsRequest<StubConfig>>) {
        for request in requests {
            match request {
                GgrsRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GgrsRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GgrsRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStub>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);

        let random_checksum: u128 = self.rng.gen();
        cell.save(frame, Some(self.gs), Some(random_checksum));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStub>) {
        self.gs = cell.load().expect("No data found.");
    }

    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        self.gs.advance_frame(inputs);
    }
}

/// A single-player game stub for tests that use `with_num_players(1)`.
/// The `advance_frame` logic only reads `inputs[0]`.
pub struct GameStub1P {
    pub gs: StateStub,
}

impl Default for GameStub1P {
    fn default() -> Self {
        Self::new()
    }
}

impl GameStub1P {
    #[allow(dead_code)]
    pub fn new() -> GameStub1P {
        GameStub1P {
            gs: StateStub { frame: 0, state: 0 },
        }
    }

    #[allow(dead_code)]
    pub fn handle_requests(&mut self, requests: Vec<GgrsRequest<StubConfig>>) {
        for request in requests {
            match request {
                GgrsRequest::LoadGameState { cell, .. } => self.load_game_state(cell),
                GgrsRequest::SaveGameState { cell, frame } => self.save_game_state(cell, frame),
                GgrsRequest::AdvanceFrame { inputs } => self.advance_frame(inputs),
            }
        }
    }

    fn save_game_state(&mut self, cell: GameStateCell<StateStub>, frame: Frame) {
        assert_eq!(self.gs.frame, frame);
        let checksum = calculate_hash(&self.gs);
        cell.save(frame, Some(self.gs), Some(checksum as u128));
    }

    fn load_game_state(&mut self, cell: GameStateCell<StateStub>) {
        self.gs = cell.load().unwrap();
    }

    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        self.gs.state += inputs[0].0.inp as i32;
        self.gs.frame += 1;
    }
}

#[derive(Default, Copy, Clone, Hash)]
pub struct StateStub {
    pub frame: i32,
    pub state: i32,
}

impl StateStub {
    fn advance_frame(&mut self, inputs: Vec<(StubInput, InputStatus)>) {
        let p0_inputs = inputs[0].0.inp;
        let p1_inputs = inputs[1].0.inp;

        if (p0_inputs + p1_inputs).is_multiple_of(2) {
            self.state += 2;
        } else {
            self.state -= 1;
        }
        self.frame += 1;
    }
}

// ── Shared session helpers ────────────────────────────────────────────────────

pub const SYNC_TIMEOUT: Duration = Duration::from_secs(2);
pub const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Shorthand for a loopback `SocketAddr` on the given port.
#[allow(dead_code)]
pub fn localhost(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

/// Build two connected `P2PSession`s (player 0 on `port1`, player 1 on `port2`).
#[allow(dead_code)]
pub fn make_p2p_sessions(
    port1: u16,
    port2: u16,
) -> (P2PSession<StubConfig>, P2PSession<StubConfig>) {
    let s1 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_player(PlayerType::Remote(localhost(port2)), 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(port1).unwrap())
        .unwrap();
    let s2 = SessionBuilder::<StubConfig>::new()
        .add_player(PlayerType::Remote(localhost(port1)), 0)
        .unwrap()
        .add_player(PlayerType::Local, 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(port2).unwrap())
        .unwrap();
    (s1, s2)
}

/// Build two connected lockstep `P2PSession`s (`max_prediction = 0`) with the given input delay.
#[allow(dead_code)]
pub fn make_lockstep_sessions(
    port1: u16,
    port2: u16,
    input_delay: usize,
) -> (P2PSession<StubConfig>, P2PSession<StubConfig>) {
    let s1 = SessionBuilder::<StubConfig>::new()
        .with_max_prediction_window(0)
        .with_input_delay(input_delay)
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_player(PlayerType::Remote(localhost(port2)), 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(port1).unwrap())
        .unwrap();
    let s2 = SessionBuilder::<StubConfig>::new()
        .with_max_prediction_window(0)
        .with_input_delay(input_delay)
        .add_player(PlayerType::Remote(localhost(port1)), 0)
        .unwrap()
        .add_player(PlayerType::Local, 1)
        .unwrap()
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(port2).unwrap())
        .unwrap();
    (s1, s2)
}

/// Poll both sessions until they reach `Running` state or the sync timeout expires.
#[allow(dead_code)]
pub fn sync_p2p_sessions<C: Config>(s1: &mut P2PSession<C>, s2: &mut P2PSession<C>) {
    let deadline = Instant::now() + SYNC_TIMEOUT;
    while Instant::now() < deadline {
        s1.poll_remote_clients();
        s2.poll_remote_clients();
        if s1.current_state() == SessionState::Running
            && s2.current_state() == SessionState::Running
        {
            return;
        }
        thread::sleep(SYNC_POLL_INTERVAL);
    }
    assert_eq!(s1.current_state(), SessionState::Running);
    assert_eq!(s2.current_state(), SessionState::Running);
}

/// Poll host and spectator until they reach `Running` state.
#[allow(dead_code)]
pub fn sync_host_and_spectator(
    host_sess: &mut P2PSession<StubConfig>,
    spec_sess: &mut SpectatorSession<StubConfig>,
) {
    let deadline = Instant::now() + SYNC_TIMEOUT;
    while Instant::now() < deadline {
        host_sess.poll_remote_clients();
        spec_sess.poll_remote_clients();
        if host_sess.current_state() == SessionState::Running
            && spec_sess.current_state() == SessionState::Running
        {
            return;
        }
        thread::sleep(SYNC_POLL_INTERVAL);
    }
    assert_eq!(host_sess.current_state(), SessionState::Running);
    assert_eq!(spec_sess.current_state(), SessionState::Running);
}

/// Build a synced host (1 local player + 1 spectator) and spectator session.
#[allow(dead_code)]
pub fn make_host_and_spectator(
    host_port: u16,
    spec_port: u16,
) -> Result<(P2PSession<StubConfig>, SpectatorSession<StubConfig>), GgrsError> {
    let mut host_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .add_player(PlayerType::Local, 0)?
        .add_player(PlayerType::Spectator(localhost(spec_port)), 1)?
        .start_p2p_session(UdpNonBlockingSocket::bind_to_port(host_port).unwrap())?;

    let mut spec_sess = SessionBuilder::<StubConfig>::new()
        .with_num_players(1)?
        .start_spectator_session(
            localhost(host_port),
            UdpNonBlockingSocket::bind_to_port(spec_port).unwrap(),
        );

    sync_host_and_spectator(&mut host_sess, &mut spec_sess);

    Ok((host_sess, spec_sess))
}
