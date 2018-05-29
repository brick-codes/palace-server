extern crate hyper;
extern crate rand;
extern crate snowflake;

mod game;
mod lobby;

use lobby::Lobby;
use std::collections::HashMap;

use snowflake::ProcessUniqueId;

#[derive(PartialEq, Eq, Hash)]
struct PlayerId(ProcessUniqueId);

#[derive(PartialEq, Eq, Hash)]
struct LobbyId(ProcessUniqueId);

impl PlayerId {
    fn new() -> PlayerId {
        PlayerId(ProcessUniqueId::new())
    }
}

impl LobbyId {
    fn new() -> LobbyId {
        LobbyId(ProcessUniqueId::new())
    }
}

fn main() {
    let lobbies: HashMap<LobbyId, Lobby> = HashMap::new();
    println!("Hello, world!");
}
