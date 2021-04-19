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

/// Represents a player in the game.  
#[derive(Debug)]
pub struct Player {
    /// The type of the player.
    pub player_type: PlayerType,
    /// The player number. Should be between 1 and the number of players
    /// in the game (e.g. in a 2 player game, either 1 or 2).
    pub player_handle: u32,
}

impl Player {
    /// Returns a person with the player number and player type given
    ///
    /// ## Arguments
    ///
    /// * `player_type` - The player type of that player
    /// * `player_handle` - The player handle of that player, should be between 0 and the number of players in the game -1.
    ///
    /// ## Examples
    ///
    /// ```
    /// use ggpo::player::{Player, PlayerType};
    /// let player = Player::new(PlayerType::Local, 0);
    /// ```
    pub fn new(player_type: PlayerType, player_handle: u32) -> Player {
        Player {
            player_handle,
            player_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_player() {
        let player = Player::new(PlayerType::Local, 0);
        assert_eq!(player.player_handle, 0);
        assert!(matches!(player.player_type, PlayerType::Local));
    }
}
