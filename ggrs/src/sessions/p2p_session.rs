use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_protocol::UdpProtocol;
use crate::network::udp_socket::NonBlockingSocket;
use crate::player;
use crate::player::{Player, PlayerType};
use crate::sync_layer::SyncLayer;
use crate::{FrameNumber, GGRSInterface, GGRSSession, PlayerHandle};
use crate::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT, NULL_FRAME};

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug, PartialEq, Eq)]
enum SessionState {
    Initializing,
    Synchronizing,
    Running,
}

#[derive(Debug)]
pub struct P2PSession {
    /// Internal State of the Session
    state: SessionState,
    /// The number of players of the session
    num_players: u32,
    /// The number of bytes an input uses
    input_size: usize,
    /// The sync layer handles player input queues, predictions
    sync_layer: SyncLayer,
    /// The time until a remote player gets disconnected
    disconnect_timeout: u32,
    /// The time until
    disconnect_notify_start: u32,
    next_recommended_sleep: u32,

    /// The UDP socket we will use to send and receive all messages
    socket: NonBlockingSocket,
    endpoints: HashMap<PlayerHandle, UdpProtocol>,

    /// This struct contains information about remote players
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
            endpoints: HashMap::new(),
        })
    }

    fn add_spectator(&mut self, player: &Player, addr: SocketAddr) -> Result<(), GGRSError> {
        todo!()
    }

    fn add_local_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        // check if valid player
        if player.player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // currently, there is no additional logic required to handle local players
        Ok(())
    }

    fn add_remote_player(&mut self, player: &Player, addr: SocketAddr) -> Result<(), GGRSError> {
        // check if valid player
        if player.player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // check if player already added
        if self.endpoints.contains_key(&player.player_handle) {
            return Err(GGRSError::InvalidHandle);
        }
        // create a udp protocol endpoint that handles all the messaging to that remote player
        let mut endpoint = UdpProtocol::new(player.player_handle, addr, self.num_players);
        endpoint.set_disconnect_notify_start(self.disconnect_notify_start);
        endpoint.set_disconnect_timeout(self.disconnect_timeout);
        self.endpoints.insert(player.player_handle, endpoint);
        // if the input delay has been set previously, erase it (remote players handle input delay at their end)
        self.sync_layer.set_frame_delay(player.player_handle, 0);
        Ok(())
    }

    fn disconnect_player_by_handle(
        &mut self,
        player_handle: PlayerHandle,
        disconnect_frame: FrameNumber,
    ) {
        assert!(self.sync_layer.current_frame() >= disconnect_frame);
        // disconnect the remote player
        self.endpoints.get_mut(&player_handle).unwrap().disconnect();

        // mark the player as disconnected
        self.local_connect_status[player_handle].disconnected = true;
        self.local_connect_status[player_handle].last_frame = disconnect_frame;

        if self.sync_layer.current_frame() > disconnect_frame {
            // TODO: the original adjusts simulation to account for the fact that the player disconnected a few frames ago,
            // resimulating with correct disconnect flags (to account for user having some AI kick in).
            // For now, we the game will have some frames with predictions instead.
        }

        // check if all remotes are synchronized now
        self.check_initial_sync();
    }

    fn check_initial_sync(&mut self) {
        // if we are not synchronizing, we don't need to do anything
        if self.state != SessionState::Synchronizing {
            return;
        }

        // if any remote player is not synchronized, we don't do anything
        if self.endpoints.values().any(|ep| !ep.is_synchronized()) {
            return;
        }

        // TODO: spectators

        // everyone is synchronized, so we can change state and accept input
        self.state = SessionState::Running;
    }
}

impl GGRSSession for P2PSession {
    fn add_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        match player.player_type {
            PlayerType::Local => return self.add_local_player(player),
            PlayerType::Remote(addr) => return self.add_remote_player(player, addr),
            PlayerType::Spectator(addr) => return self.add_spectator(player, addr),
        }
    }

    fn start_session(&mut self) -> Result<(), GGRSError> {
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }
        self.state = SessionState::Synchronizing;
        todo!()
    }

    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        // player already disconnected
        if self.local_connect_status[player_handle].disconnected {
            return Err(GGRSError::InvalidRequest);
        }

        // player is not a remote player
        if !self.endpoints.contains_key(&player_handle) {
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
        let stats = self
            .endpoints
            .get(&player_handle)
            .ok_or(GGRSError::InvalidRequest)?
            .network_stats();

        Ok(stats)
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
        // player is a known remote player
        if self.endpoints.contains_key(&player_handle) {
            return Err(GGRSError::InvalidRequest);
        }
        self.sync_layer.set_frame_delay(player_handle, frame_delay);
        Ok(())
    }

    fn set_disconnect_timeout(&mut self, timeout: u32) {
        for endpoint in self.endpoints.values_mut() {
            endpoint.set_disconnect_timeout(timeout);
        }
    }

    fn set_disconnect_notify_delay(&mut self, notify_delay: u32) {
        for endpoint in self.endpoints.values_mut() {
            endpoint.set_disconnect_notify_start(notify_delay);
        }
    }

    fn idle(&self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // do poll
        todo!()
    }
}
