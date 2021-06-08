use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_protocol::UdpProtocol;
use crate::network::udp_socket::NonBlockingSocket;
use crate::sync_layer::SyncLayer;
use crate::{FrameNumber, GGRSInterface, GGRSSession, PlayerHandle, PlayerType, NULL_FRAME};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// The minimum amounts of frames between sleeps to compensate being ahead of other players
pub const RECOMMENDATION_INTERVAL: u32 = 240;
pub const DEFAULT_DISCONNECT_TIMEOUT: u32 = 5000;
pub const DEFAULT_DISCONNECT_NOTIFY_START: u32 = 750;

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
    /// The number of players of the session.
    num_players: u32,
    /// The number of bytes an input uses.
    input_size: usize,
    /// The sync layer handles player input queues and provides predictions.
    sync_layer: SyncLayer,

    /// The time until a remote player gets disconnected.
    disconnect_timeout: u32,
    /// The time until the client will get a notification that a remote player is about to be disconnected.
    disconnect_notify_start: u32,
    /// The next frame on which the session will stop advancing frames to compensate for being before other players.
    next_recommended_sleep: u32,

    /// Internal State of the Session.
    state: SessionState,

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
            local_connect_status.push(ConnectionStatus::default());
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
        // disconnect the remote player, unwrapping is okay because the player handle was checked in disconnect_player()
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
            // For now, the game will have some frames with incorrect predictions instead.
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
            if let Player::Remote(endpoint) = player {
                if !endpoint.is_synchronized() {
                    return;
                }
            }
        }

        // TODO: spectators

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }

    fn poll_endpoints(&mut self, interface: &mut impl GGRSInterface) {
        // get all udp packets and distribute them to associated endpoints
        for (from, msg) in self.socket.receive_all_messages().iter() {
            for player in self.players.values_mut() {
                if let Player::Remote(endpoint) = player {
                    if endpoint.is_handling_message(from) {
                        endpoint.handle_message(msg);
                        break;
                    }
                }
            }
        }

        // get events from endpoints
        for player in self.players.values_mut() {
            if let Player::Remote(endpoint) = player {
                let events = endpoint.poll(&self.local_connect_status, &mut self.socket);
                // handle events
                todo!();
            }
        }
    }
}

impl GGRSSession for P2PSession {
    fn add_player(
        &mut self,
        player_type: PlayerType,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // currently, you can only add players in the init phase
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }

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

        // start the synchronisation
        self.state = SessionState::Synchronizing;
        for player in self.players.values_mut() {
            if let Player::Remote(endpoint) = player {
                endpoint.synchronize(&mut self.socket);
            }
        }
        Ok(())
    }

    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        // player already disconnected
        if self.local_connect_status[player_handle].disconnected {
            return Err(GGRSError::InvalidRequest);
        }

        // check if the player exists
        match self.players.get(&player_handle) {
            None => return Err(GGRSError::InvalidRequest),
            Some(Player::Local) => return Err(GGRSError::InvalidRequest), // TODO: disconnect individual local players?
            Some(Player::Remote(_)) => {
                let last_frame = self.local_connect_status[player_handle].last_frame;
                self.disconnect_player_by_handle(player_handle, last_frame);
                Ok(())
            }
        }
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

        // player is not a local player
        match self.players.get(&player_handle) {
            Some(Player::Local) => (),
            _ => return Err(GGRSError::InvalidRequest),
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
            self.local_connect_status[player_handle].last_frame = actual_frame;

            for player in self.players.values_mut() {
                if let Player::Remote(endpoint) = player {
                    endpoint.send_input(game_input, &self.local_connect_status, &mut self.socket);
                }
            }
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

        // receive info from remote players, rollback if necessary
        self.poll_endpoints(interface);
        Ok(())
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
            if let Player::Remote(endpoint) = player {
                endpoint.set_disconnect_timeout(timeout);
            }
        }
    }

    fn set_disconnect_notify_delay(&mut self, notify_delay: u32) {
        for player in self.players.values_mut() {
            if let Player::Remote(endpoint) = player {
                endpoint.set_disconnect_notify_start(notify_delay);
            }
        }
    }

    fn idle(&mut self, interface: &mut impl GGRSInterface) {
        self.poll_endpoints(interface);
    }
}
