use game::Game;

use ::PlayerId;

pub struct Lobby {
    players: Vec<PlayerId>,
    game: Game,
}
