#![feature(catch_expr)]

extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

mod game;

use game::GameState;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::HashMap;
use std::time::Instant;
use ws::{listen, CloseCode, Handler, Handshake, Message, Sender};

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
struct PlayerId(#[serde(serialize_with = "as_hex_str", deserialize_with = "hex_to_u128")] u128);

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
struct LobbyId(#[serde(serialize_with = "as_hex_str", deserialize_with = "hex_to_u128")] u128);

pub fn as_hex_str<T, S>(token: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: std::fmt::LowerHex,
    S: Serializer,
{
    serializer.serialize_str(&format!("{:x}", token))
}

pub fn hex_to_u128<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{Error, Unexpected};
    String::deserialize(deserializer).and_then(|string| {
        u128::from_str_radix(&string, 16)
            .map_err(|_| Error::invalid_value(Unexpected::Str(&string), &"hex encoded token"))
    })
}

struct Lobby {
    players: HashMap<PlayerId, Player>,
    max_players: u8,
    password: String,
    game: Option<GameState>,
    owner: PlayerId,
    name: String,
    creation_time: Instant,
}

impl Lobby {
    fn display(&self) -> LobbyDisplay {
        LobbyDisplay {
            cur_players: self.players.len() as u8,
            max_players: self.max_players,
            started: self.game.is_some(),
            has_password: !self.password.is_empty(),
            owner: &self.players.get(&self.owner).unwrap().name,
            name: &self.name,
            age: self.creation_time.elapsed().as_secs(),
        }
    }
}

#[derive(Serialize)]
struct LobbyDisplay<'a> {
    cur_players: u8,
    max_players: u8,
    started: bool,
    has_password: bool,
    owner: &'a str,
    name: &'a str,
    age: u64,
}

struct Player {
    name: String,
}

#[derive(Deserialize)]
struct NewLobbyMessage {
    max_players: u8,
    password: String,
    lobby_name: String,
    player_name: String,
}

#[derive(Serialize)]
struct NewLobbyResponse {
    player_id: PlayerId,
    lobby_id: LobbyId,
}

#[derive(Deserialize)]
struct JoinLobbyMessage {
    lobby_id: LobbyId,
    player_name: String,
    password: String,
}

#[derive(Serialize)]
struct JoinLobbyResponse {
    player_id: PlayerId,
}

#[derive(Deserialize)]
struct StartGameMessage {
    lobby_id: LobbyId,
    player_id: PlayerId,
}

#[derive(Deserialize)]
enum PalaceMessage {
    NewLobby(NewLobbyMessage),
    JoinLobby(JoinLobbyMessage),
    ListLobbies,
    StartGame(StartGameMessage),
}

struct Server {
    out: Sender,
    lobbies: HashMap<LobbyId, Lobby>,
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> ws::Result<()> {
        match msg {
            Message::Text(_) => self.out.close(CloseCode::Unsupported),
            Message::Binary(binary) => {
                if let Ok(message) = serde_json::from_slice::<PalaceMessage>(&binary) {
                    let response: Result<Vec<u8>, serde_json::Error> = match message {
                        PalaceMessage::NewLobby(message) => {
                            let lobby_id = LobbyId(rand::random());
                            let player_id = PlayerId(rand::random());
                            let mut players = HashMap::new();
                            players.insert(
                                player_id,
                                Player {
                                    name: message.player_name,
                                },
                            );
                            self.lobbies.insert(
                                lobby_id,
                                Lobby {
                                    players: players,
                                    game: None,
                                    password: message.password,
                                    name: message.lobby_name,
                                    owner: player_id,
                                    max_players: message.max_players,
                                    creation_time: Instant::now(),
                                },
                            );
                            serde_json::to_vec(&NewLobbyResponse {
                                player_id: player_id,
                                lobby_id: lobby_id,
                            })
                        }
                        PalaceMessage::JoinLobby(message) => {
                            let mut lobby_opt = self.lobbies.get_mut(&message.lobby_id);
                            if let Some(lobby) = lobby_opt {
                                if lobby.game.is_some() {
                                    serde_json::to_vec("game has started")
                                } else if lobby.password != message.password {
                                    serde_json::to_vec("bad password")
                                } else if lobby.players.len() as u8 >= lobby.max_players {
                                    serde_json::to_vec("lobby is full")
                                } else {
                                    let player_id = PlayerId(rand::random());
                                    lobby.players.insert(
                                        player_id,
                                        Player {
                                            name: message.player_name,
                                        },
                                    );
                                    serde_json::to_vec(&JoinLobbyResponse {
                                        player_id: player_id,
                                    })
                                }
                            } else {
                                serde_json::to_vec("lobby does not exist")
                            }
                        }
                        PalaceMessage::ListLobbies => {
                            // @Performance we should be able to serialize with Serializer::collect_seq
                            // and avoid collecting into a vector
                            serde_json::to_vec(
                                &self
                                    .lobbies
                                    .values()
                                    .map(|x| x.display())
                                    .collect::<Vec<_>>(),
                            )
                        }
                        PalaceMessage::StartGame(message) => {
                            let lobby_opt = self.lobbies.get_mut(&message.lobby_id);
                            if let Some(lobby) = lobby_opt {
                                if message.player_id != lobby.owner {
                                    serde_json::to_vec("must be the owner to start game")
                                } else if lobby.players.len() < 2 {
                                    serde_json::to_vec(
                                        "can't start a game with less than 2 players",
                                    )
                                } else {
                                    lobby.game = Some(GameState::new(lobby.players.len() as u8));
                                    serde_json::to_vec("started")
                                }
                            } else {
                                serde_json::to_vec("lobby does not exist")
                            }
                        }
                    };
                    self.out.send(ws::Message::binary(response.unwrap()))
                } else {
                    self.out.close(CloseCode::Invalid)
                }
            }
        }
    }

    fn on_close(&mut self, code: CloseCode, reason: &str) {
        // The WebSocket protocol allows for a utf8 reason for the closing state after the
        // close code. WS-RS will attempt to interpret this data as a utf8 description of the
        // reason for closing the connection. I many cases, `reason` will be an empty string.
        // So, you may not normally want to display `reason` to the user,
        // but let's assume that we know that `reason` is human-readable.
        match code {
            CloseCode::Normal => println!("The client is done with the connection."),
            CloseCode::Away => println!("The client is leaving the site."),
            _ => println!("The client encountered an error: {}", reason),
        }
    }

    fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
        self.out.send("welcome")
    }
}

fn main() {
    listen("127.0.0.1:3012", |out| Server {
        out: out,
        lobbies: HashMap::new(),
    }).unwrap()
}
