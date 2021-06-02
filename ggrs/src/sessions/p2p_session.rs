use crate::error::GGRSError;
use crate::frame_info::GameInput;
use crate::network::network_stats::NetworkStats;
use crate::network::udp_msg::ConnectionStatus;
use crate::network::udp_socket::NonBlockingSocket;
use crate::player::{Player, PlayerType};
use crate::sync_layer::SyncLayer;
use crate::{GGRSInterface, GGRSSession, PlayerHandle};
use crate::{DEFAULT_DISCONNECT_NOTIFY_START, DEFAULT_DISCONNECT_TIMEOUT, NULL_FRAME};

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
    sync_layer: SyncLayer,
    local_connect_status: Vec<ConnectionStatus>,
    disconnect_timeout: u32,
    disconnect_notify_start: u32,
    next_recommended_sleep: u32,
    socket: NonBlockingSocket,
}

impl P2PSession {
    pub fn new(num_players: u32, input_size: usize, port: u16) -> Self {
        // local connection status
        let mut local_connect_status = Vec::new();
        for _ in 0..num_players {
            local_connect_status.push(ConnectionStatus::new());
        }
        // socket address to bind to, very WIP
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port); //TODO: IpV6?
        Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            sync_layer: SyncLayer::new(num_players, input_size),
            local_connect_status,
            disconnect_timeout: DEFAULT_DISCONNECT_TIMEOUT,
            disconnect_notify_start: DEFAULT_DISCONNECT_NOTIFY_START,
            next_recommended_sleep: 0,
            socket: NonBlockingSocket::new(addr),
        }
    }

    fn add_spectator(&mut self, player: &Player) -> Result<(), GGRSError> {
        todo!()
    }

    fn add_local_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        if player.player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        Ok(())
    }

    fn add_remote_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        if player.player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        todo!()
    }
}

impl GGRSSession for P2PSession {
    /// Must be called for each player in the session (e.g. in a 3 player session, must be called 3 times).
    fn add_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        match player.player_type {
            PlayerType::Local => return self.add_local_player(player),
            PlayerType::Remote(_) => return self.add_remote_player(player),
            PlayerType::Spectator(_) => return self.add_spectator(player),
        }
    }

    /// After you are done defining and adding all players, you should start the session.
    /// # Errors
    /// Will return 'InvalidRequest' if the session has already been started before.
    fn start_session(&mut self) -> Result<(), GGRSError> {
        if self.state != SessionState::Initializing {
            return Err(GGRSError::InvalidRequest);
        }
        self.state = SessionState::Synchronizing;
        todo!()
    }

    /// Disconnects a remote player from a game.  
    /// # Errors
    ///Will return `PlayerDisconnected` if you try to disconnect a player who has already been disconnected.
    fn disconnect_player(&mut self, player_handle: PlayerHandle) -> Result<(), GGRSError> {
        todo!()
    }

    /// Used to notify GGRS of inputs that should be transmitted to remote players. `add_local_input()` must be called once every frame for all player of type `PlayerType::Local`
    /// before calling `advance_frame()`.
    fn add_local_input(
        &mut self,
        player_handle: PlayerHandle,
        input: &[u8],
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        // session is not running
        if self.state != SessionState::Running {
            return Err(GGRSError::NotSynchronized);
        }
        let mut game_input: GameInput =
            GameInput::new(self.sync_layer.current_frame(), None, self.input_size);
        game_input.copy_input(input);

        // send the input into the sync layer
        let actual_frame = self.sync_layer.add_local_input(player_handle, game_input)?;

        // if the actual frame is the null frame, the frame has been dropped by the input queues (due to changed input delay)
        if actual_frame != NULL_FRAME {
            // if not dropped, send the input to all other clients, but with the correct frame (influenced by input delay)
            game_input.frame = actual_frame;
            todo!();
        }

        Ok(())
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
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

    /// Used to fetch some statistics about the quality of the network connection.
    fn network_stats(&self, player_handle: PlayerHandle) -> Result<NetworkStats, GGRSError> {
        todo!()
    }

    /// Change the amount of frames GGRS will delay your local inputs. Must be called before the first call to `advance_frame()`.
    fn set_frame_delay(
        &mut self,
        frame_delay: u32,
        player_handle: PlayerHandle,
    ) -> Result<(), GGRSError> {
        // player handle is invalid
        if player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        self.sync_layer.set_frame_delay(player_handle, frame_delay);
        Ok(())
    }

    /// Sets the disconnect timeout.  The session will automatically disconnect from a remote peer if it has not received a packet in the timeout window.
    /// You will be notified of the disconnect.
    fn set_disconnect_timeout(&self, timeout: u32) -> Result<(), GGRSError> {
        todo!()
    }
    /// The time to wait before the first notification will be sent.
    fn set_disconnect_notify_delay(&self, notify_delay: u32) -> Result<(), GGRSError> {
        todo!()
    }

    /// Should be called periodically by your application to give GGRS a chance to do internal work. Packet transmissions and rollbacks can occur here.
    fn idle(&self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
        // do poll
        todo!()
    }
}
