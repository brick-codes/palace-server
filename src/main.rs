#![feature(plugin, decl_macro)]
#![plugin(rocket_codegen)]

#[macro_use]
extern crate rocket;
extern crate rand;
extern crate rocket_contrib;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;

mod game;

use game::GameState;
use rocket::State;
use rocket_contrib::Json;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
struct PlayerId(u128);

#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Copy)]
struct LobbyId(u128);

struct Lobby {
    players: HashMap<PlayerId, Player>,
    max_players: u8,
    password: String,
    game: Option<GameState>,
    owner: PlayerId,
    name: String,
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
enum Message {
    NewLobby(NewLobbyMessage),
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
                },
            );
            rocket::response::content::Json(
                serde_json::to_string(&NewLobbyResponse {
                    player_id: player_id,
                    lobby_id: lobby_id,
                }).unwrap(),
            )
        }
    }
}

#[get("/lobbies")]
fn lobbies(server_state: State<ServerState>) -> rocket::response::content::Json<String> {
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
