use game::GameState;

use ::PlayerId;

pub struct Lobby {
    players: Vec<PlayerId>,
    game: GameState,
}
