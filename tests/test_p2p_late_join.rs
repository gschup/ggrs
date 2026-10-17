//! Players joining a running P2P session: open slots, a joiner starting from a confirmed state,
//! and activation at a frame every peer agrees on.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use ggrs::{
    Config, DesyncDetection, Frame, GgrsError, GgrsEvent, GgrsRequest, InputStatus, Message,
    NonBlockingSocket, P2PSession, PlayerHandle, PlayerType, PredictRepeatLast, SessionBuilder,
    SessionState,
};
use serde::{Deserialize, Serialize};

const SLOTS: usize = 4;
const WINDOW: usize = 600;

#[derive(Copy, Clone, PartialEq, Default, Serialize, Deserialize, Debug)]
struct Input(u32);

/// A game whose state depends on every input, and on which slots play each frame.
#[derive(Copy, Clone, Default, Debug, PartialEq)]
struct State {
    frame: Frame,
    hash: u64,
    /// The frame each slot last started playing (after not playing), as the game saw it.
    started: [Option<Frame>; SLOTS],
    playing: [bool; SLOTS],
}

impl State {
    fn advance(&mut self, inputs: &[(Input, InputStatus)]) {
        for (slot, (input, status)) in inputs.iter().enumerate() {
            let playing = *status != InputStatus::Disconnected;
            if playing && !self.playing[slot] {
                self.started[slot] = Some(self.frame);
            }
            self.playing[slot] = playing;
            let v = if playing { input.0 as u64 + 1 } else { 0 };
            self.hash = self
                .hash
                .wrapping_mul(0x100_0000_01b3)
                .wrapping_add(v ^ (slot as u64) << 40);
        }
        self.frame += 1;
    }

    fn checksum(&self) -> u128 {
        let mut c = self.hash as u128 ^ (self.frame as u128) << 64;
        for j in self.started {
            c = c
                .wrapping_mul(31)
                .wrapping_add(j.map_or(0, |f| f as u128 + 1));
        }
        c
    }
}

struct TestConfig;

impl Config for TestConfig {
    type Input = Input;
    type InputPredictor = PredictRepeatLast;
    type State = State;
    type Address = u16;
}

/// Packets in flight between the peers of one test, delivered after `delay` ticks; a share
/// `loss` of them is dropped.
#[derive(Default)]
struct Hub {
    tick: u64,
    delay: u64,
    loss: f64,
    rng: u64,
    queues: HashMap<u16, VecDeque<(u64, u16, Message)>>,
}

impl Hub {
    fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 1_000_000) as f64 / 1_000_000.0
    }
}

struct MemSocket {
    addr: u16,
    hub: Arc<Mutex<Hub>>,
}

impl NonBlockingSocket<u16> for MemSocket {
    fn send_to(&mut self, msg: &Message, addr: &u16) {
        let mut hub = self.hub.lock().unwrap();
        if hub.loss > 0.0 && hub.random() < hub.loss {
            return;
        }
        let at = hub.tick + hub.delay;
        hub.queues
            .entry(*addr)
            .or_default()
            .push_back((at, self.addr, msg.clone()));
    }

    fn receive_all_messages(&mut self) -> Vec<(u16, Message)> {
        let mut hub = self.hub.lock().unwrap();
        let now = hub.tick;
        let queue = hub.queues.entry(self.addr).or_default();
        let mut out = Vec::new();
        while queue.front().is_some_and(|(at, ..)| *at <= now) {
            let (_, from, msg) = queue.pop_front().unwrap();
            out.push((from, msg));
        }
        out
    }
}

/// One peer: its session, its game, and what it saw.
struct Peer {
    addr: u16,
    session: P2PSession<TestConfig>,
    state: State,
    /// The last state saved for each frame (a rollback saves a frame again).
    saved: BTreeMap<Frame, State>,
    desyncs: u32,
    activations: Vec<(PlayerHandle, Frame)>,
    /// Ticks this peer could not advance (waiting for input).
    stalls: u32,
}

impl Peer {
    fn new(addr: u16, session: P2PSession<TestConfig>, state: State) -> Self {
        Peer {
            addr,
            session,
            state,
            saved: BTreeMap::new(),
            desyncs: 0,
            activations: Vec::new(),
            stalls: 0,
        }
    }

    /// Advances one frame (if the session can), with a local input that depends on the peer
    /// and the frame.
    fn step(&mut self) {
        self.session.poll_remote_clients();
        for ev in self.session.events() {
            match ev {
                GgrsEvent::DesyncDetected { .. } => self.desyncs += 1,
                GgrsEvent::PlayerActivated { player, frame } => {
                    self.activations.push((player, frame))
                }
                _ => {}
            }
        }
        if self.session.current_state() != SessionState::Running {
            return;
        }
        let frame = self.session.current_frame();
        for h in self.session.local_player_handles() {
            let input = Input((frame as u32).wrapping_mul(2654435761) >> 28 ^ self.addr as u32);
            self.session.add_local_input(h, input).unwrap();
        }
        match self.session.advance_frame() {
            Ok(requests) => self.handle(requests),
            Err(GgrsError::PredictionThreshold) => self.stalls += 1,
            Err(e) => panic!("{e}"),
        }
    }

    fn handle(&mut self, requests: Vec<GgrsRequest<TestConfig>>) {
        let mut advanced = false;
        for r in requests {
            match r {
                GgrsRequest::SaveGameState { cell, frame } => {
                    assert_eq!(self.state.frame, frame);
                    cell.save(frame, Some(self.state), Some(self.state.checksum()));
                    self.saved.insert(frame, self.state);
                }
                GgrsRequest::LoadGameState { cell, frame } => {
                    self.state = cell.load().unwrap();
                    assert_eq!(self.state.frame, frame);
                }
                GgrsRequest::AdvanceFrame { inputs } => {
                    self.state.advance(&inputs);
                    advanced = true;
                }
            }
        }
        if !advanced {
            self.stalls += 1;
        }
    }

    /// The last frame both confirmed and saved.
    fn last_confirmed(&self) -> Frame {
        self.session
            .confirmed_frame()
            .min(self.session.current_frame() - 1)
    }

    /// The state of a confirmed frame.
    fn confirmed_state(&self, frame: Frame) -> State {
        assert!(frame <= self.last_confirmed());
        self.saved[&frame]
    }
}

struct Net {
    hub: Arc<Mutex<Hub>>,
    peers: Vec<Peer>,
}

impl Net {
    fn new(delay: u64, loss: f64) -> Self {
        Net {
            hub: Arc::new(Mutex::new(Hub {
                delay,
                loss,
                rng: 0x9e37_79b9_7f4a_7c15,
                ..Hub::default()
            })),
            peers: Vec::new(),
        }
    }

    fn socket(&self, addr: u16) -> MemSocket {
        MemSocket {
            addr,
            hub: self.hub.clone(),
        }
    }

    /// A session of peers `0..n` playing slots `0..n`, the other slots open.
    fn start(&mut self, n: usize) {
        for me in 0..n {
            let mut b = builder();
            for slot in 0..SLOTS {
                b = if slot == me {
                    b.add_player(PlayerType::Local, slot).unwrap()
                } else if slot < n {
                    b.add_player(PlayerType::Remote(slot as u16), slot).unwrap()
                } else {
                    b.add_open_slot(slot).unwrap()
                };
            }
            let session = b.start_p2p_session(self.socket(me as u16)).unwrap();
            self.peers
                .push(Peer::new(me as u16, session, State::default()));
        }
    }

    /// A session of peers `0..n` playing slots `0..n`, built without late join.
    fn start_without_late_join(&mut self, n: usize) {
        for me in 0..n {
            let mut b = SessionBuilder::<TestConfig>::new()
                .with_num_players(n)
                .unwrap()
                .with_input_delay(2)
                .with_desync_detection_mode(DesyncDetection::On { interval: 10 });
            for slot in 0..n {
                b = if slot == me {
                    b.add_player(PlayerType::Local, slot).unwrap()
                } else {
                    b.add_player(PlayerType::Remote(slot as u16), slot).unwrap()
                };
            }
            let session = b.start_p2p_session(self.socket(me as u16)).unwrap();
            self.peers
                .push(Peer::new(me as u16, session, State::default()));
        }
    }

    /// One tick: every peer steps once, and a joiner catching up steps as often as it can.
    fn tick(&mut self) {
        self.hub.lock().unwrap().tick += 1;
        // GGRS resends on wall-clock timers
        std::thread::sleep(std::time::Duration::from_millis(1));
        for p in &mut self.peers {
            p.step();
            let mut extra = 0;
            while p.session.frames_behind().is_some_and(|b| b > 2) && extra < 30 {
                p.step();
                extra += 1;
            }
        }
    }

    fn run(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.tick();
        }
    }

    fn peer(&mut self, addr: u16) -> &mut Peer {
        self.peers.iter_mut().find(|p| p.addr == addr).unwrap()
    }

    /// Peer `addr` joins in `slot`: peer 0 hands it the state of its confirmed frame, every
    /// peer in `others` adds it, and it builds its session from that frame. The slots in
    /// `playing` are played (by the peer of the same address).
    fn join(&mut self, addr: u16, slot: PlayerHandle, playing: &[PlayerHandle]) -> Frame {
        let host = &self.peers[0];
        let start = host.last_confirmed();
        let state = host.confirmed_state(start);
        for p in &mut self.peers {
            p.session
                .add_remote_player(slot, addr, start)
                .expect("the joiner is added");
        }
        let mut b = builder().with_start_frame(start);
        for s in 0..SLOTS {
            b = if s == slot {
                b.add_joining_local_player(s).unwrap()
            } else if playing.contains(&s) {
                b.add_player(PlayerType::Remote(s as u16), s).unwrap()
            } else {
                b.add_open_slot(s).unwrap()
            };
        }
        let session = b.start_p2p_session(self.socket(addr)).unwrap();
        self.peers.push(Peer::new(addr, session, state));
        start
    }

    /// Runs until the joiner has caught up, then has peer 0 activate it.
    fn activate(&mut self, addr: u16, slot: PlayerHandle) -> Frame {
        self.activate_after(addr, slot, 0)
    }

    /// As `activate`, at least `margin` frames after peer 0's current frame.
    fn activate_after(&mut self, addr: u16, slot: PlayerHandle, margin: Frame) -> Frame {
        for _ in 0..3000 {
            self.tick();
            let j = self.peer(addr);
            if j.session.current_state() == SessionState::Running
                && j.session.frames_behind().is_some_and(|b| b <= 4)
                && j.session.confirmed_frame() >= j.session.current_frame() - 1
            {
                break;
            }
        }
        let j = self.peer(addr);
        assert_eq!(j.session.current_state(), SessionState::Running);
        assert!(
            j.session.frames_behind().is_some_and(|b| b <= 4),
            "the joiner caught up"
        );
        for _ in 0..200 {
            let at = self.peers[0].session.current_frame() + margin;
            match self.peers[0].session.activate_player_at(slot, at) {
                Ok(frame) => return frame,
                Err(GgrsError::NotSynchronized) => self.tick(),
                Err(e) => panic!("{e}"),
            }
        }
        panic!("the slot never became free");
    }

    /// Every pair of peers agrees on every frame both have confirmed, from the later one's
    /// start on; returns how many frames were compared.
    fn check_agree(&self) -> usize {
        let mut compared = 0;
        for a in &self.peers {
            for b in &self.peers {
                let from = a.session.start_frame().max(b.session.start_frame());
                let to = a.last_confirmed().min(b.last_confirmed());
                for f in from..=to {
                    let (sa, sb) = (a.saved.get(&f), b.saved.get(&f));
                    assert_eq!(
                        sa, sb,
                        "peers {} and {} differ at frame {f}",
                        a.addr, b.addr
                    );
                    compared += 1;
                }
            }
        }
        for p in &self.peers {
            assert_eq!(p.desyncs, 0, "peer {} reported a desync", p.addr);
        }
        compared
    }
}

fn builder() -> SessionBuilder<TestConfig> {
    SessionBuilder::<TestConfig>::new()
        .with_num_players(SLOTS)
        .unwrap()
        .with_input_delay(2)
        .with_desync_detection_mode(DesyncDetection::On { interval: 10 })
        .with_late_join(WINDOW)
}

fn joined_at(net: &Net, slot: usize) -> Vec<Option<Frame>> {
    net.peers
        .iter()
        .map(|p| p.confirmed_state(p.last_confirmed()).started[slot])
        .collect()
}

fn late_join(delay: u64, loss: f64) {
    let mut net = Net::new(delay, loss);
    net.start(2);
    net.run(120);
    let start = net.join(10, 2, &[0, 1]);
    assert!(start > 60);
    let stalls_before: Vec<u32> = net.peers[..2].iter().map(|p| p.stalls).collect();
    let frame = net.activate_after(10, 2, delay as Frame + 2);
    net.run(300);

    let compared = net.check_agree();
    assert!(compared > 1000, "only {compared} frames compared");
    // every peer saw the joiner play from the activation frame on, and not before
    assert_eq!(joined_at(&net, 2), vec![Some(frame); 3]);
    for p in &net.peers {
        assert_eq!(p.session.player_active_from(2), Some(frame));
        assert!(p.activations.contains(&(2, frame)), "peer {}", p.addr);
    }
    // the players already in barely noticed
    for (p, before) in net.peers[..2].iter().zip(stalls_before) {
        assert!(
            p.stalls - before <= 8 + 4 * delay as u32,
            "peer {} stalled {} times",
            p.addr,
            p.stalls - before
        );
    }
}

#[test]
fn test_late_joiner_plays_from_the_activation_frame() {
    late_join(0, 0.0);
}

#[test]
fn test_late_join_with_delay() {
    late_join(3, 0.0);
}

#[test]
fn test_late_join_with_delay_and_loss() {
    late_join(2, 0.05);
}

#[test]
fn test_two_players_join_at_once() {
    let mut net = Net::new(1, 0.02);
    net.start(2);
    net.run(60);
    net.join(10, 2, &[0, 1]);
    net.join(11, 3, &[0, 1]);
    // the second joiner adds the first (who was added to it as it joined)
    let start_a = net.peer(10).session.start_frame();
    net.peer(11)
        .session
        .add_remote_player(2, 10, start_a)
        .unwrap();
    let fa = net.activate(10, 2);
    let fb = net.activate(11, 3);
    net.run(300);
    net.check_agree();
    assert_eq!(joined_at(&net, 2), vec![Some(fa); 4]);
    assert_eq!(joined_at(&net, 3), vec![Some(fb); 4]);
}

#[test]
fn test_join_a_session_played_alone() {
    // a host with no one to send to keeps its inputs for whoever joins
    let mut net = Net::new(1, 0.0);
    net.start(1);
    net.run(100);
    net.join(10, 1, &[0]);
    let frame = net.activate(10, 1);
    net.run(200);
    assert!(net.check_agree() > 200);
    assert_eq!(joined_at(&net, 1), vec![Some(frame); 2]);
}

#[test]
fn test_a_slot_is_played_again_after_its_player_left() {
    let mut net = Net::new(1, 0.0);
    net.start(3);
    net.run(100);
    // peer 2 leaves: everyone else drops it
    net.peers.retain(|p| p.addr != 2);
    for p in &mut net.peers {
        p.session.disconnect_player(2).unwrap();
    }
    net.run(60);
    net.join(10, 2, &[0, 1]);
    let frame = net.activate(10, 2);
    net.run(300);
    net.check_agree();
    for p in &net.peers {
        assert_eq!(p.session.player_active_from(2), Some(frame));
    }
    // the game saw the slot go quiet when its player left, and start again with the new one
    assert_eq!(joined_at(&net, 2), vec![Some(frame); 3]);
    let host = &net.peers[0];
    assert!(!host.confirmed_state(frame).playing[2]);
}

#[test]
fn test_activation_errors() -> Result<(), GgrsError> {
    let mut net = Net::new(0, 0.0);
    net.start(2);
    net.run(30);
    let s = &mut net.peers[0].session;
    // nobody in the slot
    assert!(matches!(
        s.activate_player(2),
        Err(GgrsError::InvalidRequest { .. })
    ));
    // a played slot
    assert!(s.activate_player(1).is_err());
    assert!(s.add_remote_player(1, 10, 0).is_err());
    // not a slot
    assert!(s.add_remote_player(SLOTS, 10, 0).is_err());
    // an address already in use
    assert!(s.add_remote_player(2, 1, 0).is_err());
    // a player added but not active plays no frame
    s.add_remote_player(2, 10, 20)?;
    assert_eq!(s.player_active_from(2), None);
    // nor can they be activated before we can exchange input with them
    assert!(matches!(
        s.activate_player(2),
        Err(GgrsError::NotSynchronized)
    ));
    // dropping them frees the slot
    s.disconnect_player(2)?;
    s.add_remote_player(2, 11, 20)?;
    Ok(())
}

#[test]
fn test_inputs_from_beyond_the_window_are_refused() {
    let mut net = Net::new(0, 0.0);
    net.start(2);
    net.run(WINDOW + 100);
    let s = &mut net.peers[0].session;
    assert!(s.add_remote_player(2, 10, 5).is_err());
    let recent = s.confirmed_frame();
    assert!(s.add_remote_player(2, 10, recent).is_ok());
}

#[test]
fn test_open_slots_play_as_disconnected() -> Result<(), GgrsError> {
    // open slots are disconnected players to the game
    let hub = Arc::new(Mutex::new(Hub::default()));
    let mut s = SessionBuilder::<TestConfig>::new()
        .with_num_players(3)?
        .add_player(PlayerType::Local, 0)?
        .add_open_slot(1)?
        .add_open_slot(2)?
        .with_late_join(WINDOW)
        .start_p2p_session(MemSocket { addr: 0, hub })?;
    assert_eq!(s.current_state(), SessionState::Running);
    s.add_local_input(0, Input(1))?;
    let requests = s.advance_frame()?;
    let inputs = requests
        .into_iter()
        .find_map(|r| match r {
            GgrsRequest::AdvanceFrame { inputs } => Some(inputs),
            _ => None,
        })
        .unwrap();
    assert_eq!(inputs[1].1, InputStatus::Disconnected);
    assert_eq!(inputs[2].1, InputStatus::Disconnected);
    // the inputs were kept for a player who joins late
    assert!(s.add_remote_player(1, 5, 0).is_ok());
    Ok(())
}

#[test]
fn test_without_late_join_players_cannot_join() -> Result<(), GgrsError> {
    let mut net = Net::new(0, 0.0);
    net.start_without_late_join(2);
    net.run(30);
    assert!(net.check_agree() > 0);
    let s = &mut net.peers[0].session;
    assert!(matches!(
        s.add_remote_player(2, 10, 0),
        Err(GgrsError::InvalidRequest { .. })
    ));
    assert!(matches!(
        s.activate_player(1),
        Err(GgrsError::InvalidRequest { .. })
    ));
    Ok(())
}

#[test]
fn test_builder_late_join_errors() {
    let b = || {
        SessionBuilder::<TestConfig>::new()
            .with_num_players(2)
            .unwrap()
    };
    assert!(b().add_open_slot(2).is_err());
    assert!(b()
        .add_player(PlayerType::Local, 0)
        .unwrap()
        .add_open_slot(0)
        .is_err());
    assert!(b().add_open_slot(1).unwrap().add_open_slot(1).is_err());
    assert!(b()
        .add_joining_local_player(1)
        .unwrap()
        .add_open_slot(1)
        .is_err());
    // shrinking below an open slot
    assert!(b().add_open_slot(1).unwrap().with_num_players(1).is_err());
    let hub = Arc::new(Mutex::new(Hub::default()));
    let start = |b: SessionBuilder<TestConfig>| {
        b.start_p2p_session(MemSocket {
            addr: 0,
            hub: hub.clone(),
        })
    };
    let local = || b().add_player(PlayerType::Local, 0).unwrap();
    match start(local().add_open_slot(1).unwrap().with_start_frame(10)) {
        Err(GgrsError::InvalidRequest { info }) => assert!(info.contains("late join")),
        _ => panic!("expected an InvalidRequest"),
    }
    // open slots, joining players and start frames need late join
    assert!(start(local().add_open_slot(1).unwrap()).is_err());
    assert!(start(local().add_joining_local_player(1).unwrap()).is_err());
    assert!(start(
        local()
            .add_player(PlayerType::Remote(1), 1)
            .unwrap()
            .with_start_frame(10)
    )
    .is_err());
    // a session that starts late cannot have late spectators
    assert!(start(
        local()
            .add_open_slot(1)
            .unwrap()
            .with_start_frame(10)
            .with_late_join(WINDOW)
            .with_late_spectators(true)
    )
    .is_err());
    assert!(start(local().add_open_slot(1).unwrap().with_late_join(WINDOW)).is_ok());
}

#[test]
fn test_a_joiner_far_behind_catches_up() {
    // the joiner takes a while to load: the others play 300 frames past its start frame
    let mut net = Net::new(1, 0.01);
    net.start(2);
    net.run(60);
    let start = net.join(10, 2, &[0, 1]);
    let joiner = net.peers.pop().unwrap();
    net.run(300);
    net.peers.push(joiner);
    let frame = net.activate(10, 2);
    assert!(frame > start + 300);
    net.run(200);
    assert!(net.check_agree() > 1000);
    assert_eq!(joined_at(&net, 2), vec![Some(frame); 3]);
}
