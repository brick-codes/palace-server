#![feature(plugin, decl_macro)]
#![plugin(rocket_codegen)]

#[macro_use] extern crate rocket;
extern crate rand;
extern crate snowflake;

#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

mod game;
mod lobby;

use lobby::Lobby;
use snowflake::ProcessUniqueId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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

struct Player {
    active_lobby: LobbyId,
    name: String
}

struct ServerState {
    lobbies: HashMap<LobbyId, Lobby>,
    players: HashMap<PlayerId, Player>
}

#[derive(Deserialize)]
struct NewUserMessage {
    name: String
}

#[derive(Deserialize)]
enum Message {
    NewUser(NewUserMessage)
}

fn palace_serve(req: Request<Body>) -> Response<Body> {
    let a = unsafe { SERVER_STATE.clone().unwrap() };
    match (req.method(), req.uri().path()) {
        (&Method::GET, _) => {
            let new_game = game::GameState::new(4);
            let pub_state = new_game.public_state();
            let serialized = ::serde_json::to_string(&pub_state).unwrap();
            Response::new(Body::from(serialized))
        },
        (&Method::POST, "/api") => {
            // Deserialize Message with serde
            let m: Message = serde_json::from_slice::<Message>(req.body()).unwrap();
            Response::new("hello".into())
        },
        _ => {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::NOT_FOUND;
            res
        }
    }
}

#[post("/", data = "<message>")]
fn api(server_state: State<T>, message: Json<Message>) -> &'static str  {
    match message {
        Message::NewUserMessage => {
            "hello"
        }
    }
}

fn main() {
    // @Performance should be a concurrent hash map, FMV hashing could be good as well
    let server_state = Arc::new(RwLock::new(ServerState {
        lobbies: HashMap::new(),
        players: HashMap::new(),
    }));

    rocket::ignite().manage(server_state).mount("/", routes![index]).launch();
}
