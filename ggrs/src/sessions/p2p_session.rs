use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_protocol::UdpProtocol;
use crate::network::udp_socket::NonBlockingSocket;
use crate::sync_layer::SyncLayer;
use crate::{FrameNumber, GGRSInterface, GGRSSession, PlayerHandle, PlayerType};
use crate::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT, NULL_FRAME};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug, PartialEq, Eq)]
enum SessionState {
    Initializing,
    Synchronizing,
    Running,
}

#[derive(Debug, PartialEq, Eq)]
enum Player {
    Local,
    Remote(UdpProtocol),
}

#[derive(Debug)]
pub struct P2PSession {
    /// Internal State of the Session.
    state: SessionState,
    /// The number of players of the session.
    num_players: u32,
    /// The number of bytes an input uses.
    input_size: usize,
    /// The sync layer handles player input queues and provides predictions.
    sync_layer: SyncLayer,
    /// The time until a remote player gets disconnected.
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    /// The next frame on which the session will stop advancing frames to compensate for being before other players.
    next_recommended_sleep: u32,

    /// The `P2PSession` uses this UDP socket to send and receive all messages for remote players.
    socket: NonBlockingSocket,
    /// A map of player handle to a player struct that handles receiving and sending messages for remote players and register local players.
    players: HashMap<PlayerHandle, Player>,
    /// This struct contains information about remote players, like connection status and the frame of last received input.
    local_connect_status: Vec<ConnectionStatus>,
}

impl P2PSession {
    pub(crate) fn new(
        num_players: u32,
        input_size: usize,
        port: u16,
    ) -> Result<Self, std::io::Error> {
        // local connection status
        let mut local_connect_status = Vec::new();
        for _ in 0..num_players {
            local_connect_status.push(ConnectionStatus::new());
        }

        // udp nonblocking socket creation
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port); //TODO: IpV6?
        let socket = NonBlockingSocket::new(addr)?;

        Ok(Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            socket,
            local_connect_status,
            sync_layer: SyncLayer::new(num_players, input_size),
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            next_recommended_sleep: 0,
            players: HashMap::new(),
        })
    }

    fn add_local_player(&mut self, player_handle: PlayerHandle) {
        self.players.insert(player_handle, Player::Local);
    }

    fn add_remote_player(&mut self, player_handle: PlayerHandle, addr: SocketAddr) {
        // create a udp protocol endpoint that handles all the messaging to that remote player
        let mut endpoint = UdpProtocol::new(player_handle, addr, self.num_players);
        endpoint.set_disconnect_notify_start(self.disconnect_notify_start);
        endpoint.set_disconnect_timeout(self.disconnect_timeout);
        self.players.insert(player_handle, Player::Remote(endpoint));
        // if the input delay has been set previously, erase it (remote players handle input delay at their end)
        self.sync_layer.set_frame_delay(player_handle, 0);
    }

    fn add_spectator(&mut self, player_handle: PlayerHandle, addr: SocketAddr) {
        todo!()
    }

    fn disconnect_player_by_handle(
        &mut self,
        player_handle: PlayerHandle,
        disconnect_frame: FrameNumber,
    ) {
        assert!(self.sync_layer.current_frame() >= disconnect_frame);
        // disconnect the remote player
        match self.players.get_mut(&player_handle).unwrap() {
            Player::Remote(endpoint) => endpoint.disconnect(),
            Player::Local => (),
        }

        // mark the player as disconnected
        self.local_connect_status[player_handle].disconnected = true;
        self.local_connect_status[player_handle].last_frame = disconnect_frame;

        if self.sync_layer.current_frame() > disconnect_frame {
            // TODO: pond3r/ggpo adjusts simulation to account for the fact that the player disconnected a few frames ago,
            // resimulating with correct disconnect flags (to account for user having some AI kick in).
            // For now, we the game will have some frames with incorrect predictions instead.
        }

        // check if all remotes are synchronized now
        self.check_initial_sync();
    }

    fn check_initial_sync(&mut self) {
        // if we are not synchronizing, we don't need to do anything
        if self.state != SessionState::Synchronizing {
            return;
        }

        // if any remote player is not synchronized, we continue synchronizing
        for player in self.players.values() {
            match player {
                Player::Remote(endpoint) => {
                    if !endpoint.is_synchronized() {
                        return;
                    }
                }
                Player::Local => (),
            }
        }

        // TODO: spectators

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }
}

impl GGRSSession for P2PSession {
    fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // check if valid player
        if player_handle >= self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // check if player handle already exists
        if self.players.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // add the player depending on type
        match player_type {
            PlayerType::Local => self.add_local_player(player_handle),
            PlayerType::Remote(addr) => self.add_remote_player(player_handle, addr),
            PlayerType::Spectator(addr) => self.add_spectator(player_handle, addr),
        }
        Ok(())
    }

    fn start_session(&mut self) -> Result<(), GGRSError> {
        // if we are not in the initialization state, we already started the session at some point
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

        // check if the amount of players is correct
        if self.players.len() != self.num_players as usize {
            return Err(GGRSError::InvalidRequest);
        }

        // start the session
        self.state = SessionState::Synchronizing;
        Ok(())
    }

    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        // player already disconnected
        if self.local_connect_status[player_handle].disconnected {
            return Err(GGRSError::InvalidRequest);
        }

        // player handle is not registered
        if !self.players.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }

        // disconnect the player
        let frame_to = self.local_connect_status[player_handle].last_frame;
        self.disconnect_player_by_handle(player_handle, frame_to);
        Ok(())
    }

    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        // session is not running and synchronzied
        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }

        //create an input struct for current frame
        let mut game_input: GameInput =
            GameInput::new(self.sync_layer.current_frame(), None, self.input_size);
        game_input.copy_input(input);

        // send the input into the sync layer
        let actual_frame = self.sync_layer.add_local_input(player_handle, game_input)?;

        // if the actual frame is the null frame, the frame has been dropped by the input queues (for example due to changed input delay)
        if actual_frame != NULL_FRAME {
            // if not dropped, send the input to all other clients, but with the correct frame (influenced by input delay)
            game_input.frame = actual_frame;
            todo!();
        }

        Ok(())
    }

    fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // save the current frame in the syncronization layer
        self.sync_layer
            .save_current_state(interface.save_game_state());
        // get correct inputs for the current frame
        let sync_inputs = self.sync_layer.synchronized_inputs();
        for input in &sync_inputs {
            assert_eq!(input.frame, self.sync_layer.current_frame());
        }
        // advance the frame
        self.sync_layer.advance_frame();
        interface.advance_frame(sync_inputs);

        // do poll
        todo!()
    }

    fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // returns the network stats. If the player does not exists, return an error.
        match self
            .players
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
        {
            Player::Local => return Err(GGRSError::InvalidRequest),
            Player::Remote(endpoint) => return Ok(endpoint.network_stats()),
        }
    }

    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }

        match self
            .players
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
        {
            Player::Remote(_) => return Err(GGRSError::InvalidRequest),
            Player::Local => {
                self.sync_layer.set_frame_delay(player_handle, frame_delay);
                Ok(())
            }
        }
    }

    fn set_disconnect_timeout(&mut self, timeout: u32) {
        for player in self.players.values_mut() {
            match player {
                Player::Remote(endpoint) => endpoint.set_disconnect_timeout(timeout),
                Player::Local => (),
            }
        }
    }

    fn set_disconnect_notify_delay(&mut self, notify_delay: u32) {
        for player in self.players.values_mut() {
            match player {
                Player::Remote(endpoint) => endpoint.set_disconnect_notify_start(notify_delay),
                Player::Local => (),
            }
        }
    }

    fn idle(&self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // do poll
        todo!()
    }
}
