use crate::error::GGRSError;
use crate::network_stats::NetworkStats;
use crate::player::Player;
use crate::player::PlayerType;
use crate::sync_layer::SyncLayer;
use crate::{GGRSInterface, GGRSSession, PlayerHandle};

enum SessionState {
    Initializing,
    Synchronizing,
    Running,
}

pub struct P2PSession {
    state: SessionState,
    num_players: u32,
    input_size: usize,
    sync_layer: SyncLayer,
}

impl P2PSession {
    pub fn new(num_players: u32, input_size: usize) -> Self {
        Self {
            state: SessionState::Initializing,
            num_players,
            input_size,
            sync_layer: SyncLayer::new(num_players, input_size),
        }
    }

    fn add_spectator(&mut self, player: &Player) -> Result<(), GGRSError> {
        todo!()
    }

    fn add_local_player(&mut self, player: &Player) -> Result<(), GGRSError> {
        if player.player_handle > self.num_players as PlayerHandle {
            return Err(GGRSError::InvalidHandle);
        }
        todo!()
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

    /// After you are done defining and adding all players, you should start the session
    fn start_session(&mut self) -> Result<(), GGRSError> {
        todo!()
    }

    /// Disconnects a remote player from a game.  
    /// # Errors
    ///Will return a `PlayerDisconnectedError` if you try to disconnect a player who has already been disconnected.
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
        todo!()
    }

    /// You should call this to notify GGRS that you are ready to advance your gamestate by a single frame. Don't advance your game state through any other means than this.
    fn advance_frame(&mut self, interface: &mut impl GGRSInterface) -> Result<(), GGRSError> {
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
        todo!()
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
        todo!()
    }
}
