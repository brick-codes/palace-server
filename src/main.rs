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
    players: Vec<PlayerId>,
    game: GameState,
}

struct Player {
    active_lobby: Option<LobbyId>,
    name: String,
    last_active: Instant,
}

struct ServerState {
    lobbies: RwLock<HashMap<LobbyId, Lobby>>,
    players: RwLock<HashMap<PlayerId, Player>>,
}

#[derive(Deserialize)]
struct NewUserMessage {
    name: String,
}

#[derive(Deserialize)]
struct NewLobbyMessage {
    player_id: PlayerId,
    num_players: u8,
}

#[derive(Deserialize)]
enum Message {
    NewUser(NewUserMessage),
    NewLobby(NewLobbyMessage),
}

#[post("/api", format = "application/json", data = "<message>")]
fn api(
    server_state: State<ServerState>,
    message: Json<Message>,
) -> rocket::response::content::Json<String> {
    match message.into_inner() {
        Message::NewUser(message) => {
            let player_id = PlayerId(rand::random());
            server_state.players.write().unwrap().insert(
                player_id,
                Player {
                    name: message.name,
                    active_lobby: None,
                    last_active: Instant::now(),
                },
            );
            rocket::response::content::Json(serde_json::to_string(&player_id).unwrap())
        }
        Message::NewLobby(message) => {
            let lobby_id = LobbyId(rand::random());
            let mut players = server_state.players.write().unwrap();
            let mut lobbies = server_state.lobbies.write().unwrap();
            // TODO: assert player is actually valid
            let player = players.get_mut(&message.player_id).unwrap();
            if let Some(old_lobby_id) = player.active_lobby {
                let old_lobby_remaining_players = {
                    let old_lobby = lobbies.get_mut(&old_lobby_id).unwrap();
                    old_lobby.players = old_lobby
                        .players
                        .iter()
                        .filter(|x| *x != &message.player_id)
                        .map(|x| *x)
                        .collect();
                    old_lobby.players.len()
                };
                if old_lobby_remaining_players == 0 {
                    lobbies.remove(&old_lobby_id);
                }
            }
            player.last_active = Instant::now();
            player.active_lobby = Some(lobby_id);
            lobbies.insert(
                lobby_id,
                Lobby {
                    players: vec![message.player_id],
                    game: GameState::new(message.num_players),
                },
            );
            rocket::response::content::Json(serde_json::to_string(&lobby_id).unwrap())
        }
    }
}

#[get("/lobbies")]
fn lobbies(server_state: State<ServerState>) -> Json<Vec<LobbyId>> {
    Json(
        server_state
            .lobbies
            .read()
            .unwrap()
            .keys()
            .map(|x| *x)
            .collect(),
    )
}

fn main() {
    // @Performance should be a concurrent hash map, FNV hashing could be good as well
    let server_state = ServerState {
        lobbies: RwLock::new(HashMap::new()),
        players: RwLock::new(HashMap::new()),
    };

    rocket::ignite()
        .manage(server_state)
        .mount("/", routes![api, lobbies])
        .launch();
}
