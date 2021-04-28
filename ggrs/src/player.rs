use crate::PlayerHandle;
/// Defines the three types of players that can exist: local player, who play on the local device,
/// remote players, who play on other devices and spectators, who are remote players that do not contribute to the game input.
/// Both Remote and Spectator have a socket address associated with them.
#[derive(Debug)]
pub enum PlayerType {
    /// This player plays on the local device
    Local,
    /// This player plays on a remote device identified by the socket address
    Remote(std::net::SocketAddr),
    /// This player spectates on a remote device identified by the socket address. They do not contribute to the game input.
    Spectator(std::net::SocketAddr),
}

impl Default for PlayerType {
    fn default() -> Self {
        PlayerType::Local
    }
}

/// Represents a player in the game.  
#[derive(Debug, Default)]
pub struct Player {
    /// The type of the player.
    pub player_type: PlayerType,
    /// The player number. The player handle should be between 0 and the number of players in the game - 1 (e.g. in a 2 player game, either 0 or 1).
    pub player_handle: PlayerHandle,
}

impl Player {
    /// Returns a person with the player handle and player type given. The player handle should be between 0 and the number of players in the game - 1.
    /// # Examples
    ///
    /// ```
    /// use ggrs::player::{Player, PlayerType};
    /// let player = Player::new(PlayerType::Local, 0);
    /// ```
    pub fn new(player_type: PlayerType, player_handle: PlayerHandle) -> Player {
        Player {
            player_handle,
            player_type,
        }
    }
}
