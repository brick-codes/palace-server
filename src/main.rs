#![feature(plugin, decl_macro)]
#![plugin(rocket_codegen)]

extern crate rand;
extern crate rocket;
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod game;

use game::GameState;
use rocket::State;
use rocket_contrib::Json;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

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

struct ServerState {
    lobbies: RwLock<HashMap<LobbyId, Lobby>>,
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
enum Message {
    NewLobby(NewLobbyMessage),
    JoinLobby(JoinLobbyMessage),
    StartGame(StartGameMessage),
}

#[post("/api", format = "application/json", data = "<message>")]
fn api(
    server_state: State<ServerState>,
    message: Json<Message>,
) -> rocket::response::content::Json<String> {
    match message.into_inner() {
        Message::NewLobby(message) => {
            let lobby_id = LobbyId(rand::random());
            let player_id = PlayerId(rand::random());
            let mut lobbies = server_state.lobbies.write().unwrap();
            let mut players = HashMap::new();
            players.insert(
                player_id,
                Player {
                    name: message.player_name,
                },
            );
            lobbies.insert(
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
            rocket::response::content::Json(
                serde_json::to_string(&NewLobbyResponse {
                    player_id: player_id,
                    lobby_id: lobby_id,
                }).unwrap(),
            )
        }
        Message::JoinLobby(message) => {
            let mut lobbies = server_state.lobbies.write().unwrap();
            let mut lobby_opt = lobbies.get_mut(&message.lobby_id);
            if let Some(lobby) = lobby_opt {
                if lobby.game.is_some() {
                    return rocket::response::content::Json("game has started".into());
                }
                if lobby.password != message.password {
                    return rocket::response::content::Json("bad password".into());
                }
                if lobby.players.len() as u8 >= lobby.max_players {
                    return rocket::response::content::Json("lobby is full".into());
                }
                let player_id = PlayerId(rand::random());
                lobby.players.insert(
                    player_id,
                    Player {
                        name: message.player_name,
                    },
                );
                rocket::response::content::Json(
                    serde_json::to_string(&JoinLobbyResponse {
                        player_id: player_id,
                    }).unwrap(),
                )
            } else {
                rocket::response::content::Json("lobby does not exist".into())
            }
        }
        Message::StartGame(message) => {
            let mut lobbies = server_state.lobbies.write().unwrap();
            let lobby_opt = lobbies.get_mut(&message.lobby_id);
            if let Some(lobby) = lobby_opt {
                if message.player_id != lobby.owner {
                    return rocket::response::content::Json(
                        "must be the owner to start game".into(),
                    );
                }
                if lobby.players.len() < 2 {
                    return rocket::response::content::Json(
                        "can't start a game with less than 2 players".into(),
                    );
                }
                lobby.game = Some(GameState::new(lobby.players.len() as u8));
                rocket::response::content::Json("started".into())
            } else {
                rocket::response::content::Json("lobby does not exist".into())
            }
        }
    }
}

#[get("/lobbies")]
fn lobbies(server_state: State<ServerState>) -> rocket::response::content::Json<String> {
    // @Performance we should be able to serialize with Serializer::collect_seq
    // and avoid collecting into a vector
    rocket::response::content::Json(
        serde_json::to_string(
            &server_state
                .lobbies
                .read()
                .unwrap()
                .values()
                .map(|x| x.display())
                .collect::<Vec<_>>(),
        ).unwrap(),
    )
}

fn main() {
    // @Performance should be a concurrent hash map, FNV hashing could be good as well
    let server_state = ServerState {
        lobbies: RwLock::new(HashMap::new()),
    };

    rocket::ignite()
        .manage(server_state)
        .mount("/", routes![api, lobbies])
        .launch();
}
