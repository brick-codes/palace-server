extern crate hyper;
extern crate rand;
extern crate snowflake;

#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;

mod game;
mod lobby;

use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper::service::service_fn_ok;
use hyper::rt::Future;
use lobby::Lobby;
use snowflake::ProcessUniqueId;
use std::collections::HashMap;
use std::sync::RwLock;

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
    active_lobby: LobbyId
}

struct ServerState {
    lobbies: HashMap<LobbyId, Lobby>,
    players: HashMap<PlayerId, Player>
}

fn palace_serve(req: Request<Body>, server_state: &RwLock<ServerState>) -> Response<Body> {
   match (req.method(), req.uri().path()) {
        (&Method::GET, _) => {
            Response::new(Body::from("hello"))
        },
        _ => {
            let mut res = Response::new(Body::empty());
            *res.status_mut() = StatusCode::NOT_FOUND;
            res
        }
    }
}

fn main() {
    // TODO PERF could be FmvMaps, test perf
    let server_state = RwLock::new(ServerState {
        lobbies: HashMap::new(),
        players: HashMap::new(),
    });

    let addr = ([127, 0, 0, 1], 80).into();

    let server = Server::bind(&addr)
        .serve(|| service_fn_ok(|req| { palace_serve(req, &server_state) }))
        .map_err(|e| eprintln!("server error: {}", e));

    println!("Listening on http://{}", addr);

    hyper::rt::run(server);
}
