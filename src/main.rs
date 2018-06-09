#![feature(catch_expr)]

extern crate either;
extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

mod game;

use either::Either;
use game::GameState;
use rand::Rng;
use serde::{Deserialize, Deserializer, Serializer};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;
use ws::{CloseCode, Handler, Handshake, Message, Sender};

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
    connection: Either<Sender, Instant>,
    turn_number: u8,
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
struct ChooseFaceupMessage {
    lobby_id: LobbyId,
    player_id: PlayerId,
    card_one: game::Card,
    card_two: game::Card,
    card_three: game::Card,
}

#[derive(Deserialize)]
enum PalaceMessage {
    NewLobby(NewLobbyMessage),
    JoinLobby(JoinLobbyMessage),
    ListLobbies,
    StartGame(StartGameMessage),
    ChooseFaceup(ChooseFaceupMessage),
}

#[derive(Serialize)]
enum PalaceOutMessage<'a> {
    NewLobbyResponse(&'a NewLobbyResponse),
    JoinLobbyResponse(&'a JoinLobbyResponse),
    LobbyList(&'a [LobbyDisplay<'a>]),
}

struct Server {
    out: Sender,
    lobbies: Rc<RefCell<HashMap<LobbyId, Lobby>>>,
    connected_lobby_player: Option<(LobbyId, PlayerId)>,
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> ws::Result<()> {
        println!("got message");
        match msg {
            Message::Text(_) => self.out.close(CloseCode::Unsupported),
            Message::Binary(binary) => {
                /*
                unsafe {
                    let string = String::from_utf8_unchecked(binary.clone());
                    println!("{}", string);
                    let pm: PalaceMessage = serde_json::from_str(&string).unwrap();
                } */
                if let Ok(message) = serde_json::from_slice::<PalaceMessage>(&binary) {
                    let response: Result<Vec<u8>, serde_json::Error> = match message {
                        PalaceMessage::NewLobby(message) => {
                            let mut lobbies = self.lobbies.borrow_mut();
                            let lobby_id = LobbyId(rand::random());
                            let player_id = PlayerId(rand::random());
                            let mut players = HashMap::new();
                            players.insert(
                                player_id,
                                Player {
                                    name: message.player_name,
                                    connection: either::Left(self.out.clone()),
                                    turn_number: 0,
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
                            self.connected_lobby_player = Some((lobby_id, player_id));
                            serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&NewLobbyResponse {
                                player_id: player_id,
                                lobby_id: lobby_id,
                            }))
                        }
                        PalaceMessage::JoinLobby(message) => {
                            let mut lobbies = self.lobbies.borrow_mut();
                            let mut lobby_opt = lobbies.get_mut(&message.lobby_id);
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
                                            connection: Either::Left(self.out.clone()),
                                            turn_number: 0,
                                        },
                                    );
                                    self.connected_lobby_player =
                                        Some((message.lobby_id, player_id));
                                    serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(
                                        &JoinLobbyResponse {
                                            player_id: player_id,
                                        },
                                    ))
                                }
                            } else {
                                serde_json::to_vec("lobby does not exist")
                            }
                        }
                        PalaceMessage::ListLobbies => {
                            println!("got lobby message");
                            let mut lobbies = self.lobbies.borrow_mut();
                            // @Performance we should be able to serialize with Serializer::collect_seq
                            // and avoid collecting into a vector
                            serde_json::to_vec(&PalaceOutMessage::LobbyList(
                                &lobbies.values().map(|x| x.display()).collect::<Vec<_>>(),
                            ))
                        }
                        PalaceMessage::StartGame(message) => {
                            let mut lobbies = self.lobbies.borrow_mut();
                            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
                                if message.player_id != lobby.owner {
                                    serde_json::to_vec("must be the owner to start game")
                                } else if lobby.players.len() < 2 {
                                    serde_json::to_vec(
                                        "can't start a game with less than 2 players",
                                    )
                                } else {
                                    let num_players = lobby.players.len() as u8;
                                    let gs = GameState::new(num_players);
                                    let public_gs_json =
                                        serde_json::to_vec(&gs.public_state()).unwrap();
                                    lobby.game = Some(gs);
                                    let mut turn_numbers: Vec<
                                        u8,
                                    > = (0..num_players).collect();
                                    rand::thread_rng().shuffle(&mut turn_numbers);
                                    let mut turn_numbers = turn_numbers.into_iter();
                                    for player in lobby.players.values_mut() {
                                        player.turn_number = turn_numbers.next().unwrap();
                                        match player.connection {
                                            either::Left(ref mut sender) => {
                                                sender.send(public_gs_json.clone())?;
                                                match lobby.game {
                                                    Some(ref mut gs) => {
                                                        sender.send(serde_json::to_vec(gs.get_hand(player.turn_number)).unwrap())?;
                                                    }
                                                    None => unreachable!(),
                                                }
                                            }
                                            either::Right(_) => (),
                                        }
                                    }
                                    return Ok(());
                                }
                            } else {
                                serde_json::to_vec("lobby does not exist")
                            }
                        }
                        PalaceMessage::ChooseFaceup(message) => {
                            let mut lobbies = self.lobbies.borrow_mut();
                            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
                                if let Some(ref mut gs) = lobby.game {
                                    let result;
                                    if let Some(player) = lobby.players.get(&message.player_id) {
                                        if player.turn_number == gs.active_player {
                                            return self.out.send(
                                                serde_json::to_vec("it is not your turn").unwrap(),
                                            );
                                        }
                                        result = gs.choose_three_faceup(
                                            message.card_one,
                                            message.card_two,
                                            message.card_three,
                                        );
                                    } else {
                                        return self.out.send(
                                            serde_json::to_vec("player does not exist").unwrap(),
                                        );
                                    }
                                    if result.is_ok() {
                                        let public_gs_json =
                                            serde_json::to_vec(&gs.public_state()).unwrap();
                                        for player in lobby.players.values_mut() {
                                            match player.connection {
                                                either::Left(ref mut sender) => {
                                                    sender.send(public_gs_json.clone())?;
                                                    sender.send(serde_json::to_vec(gs.get_hand(player.turn_number)).unwrap())?;
                                                }
                                                either::Right(_) => (),
                                            }
                                        }
                                        return Ok(());
                                    } else {
                                        return self
                                            .out
                                            .send(serde_json::to_vec("illegal").unwrap());
                                    }
                                } else {
                                    return self
                                        .out
                                        .send(serde_json::to_vec("game has not started").unwrap());
                                }
                            } else {
                                return self
                                    .out
                                    .send(serde_json::to_vec("lobby does not exist").unwrap());
                            }
                        }
                    };
                    self.out.send(ws::Message::binary(response.unwrap()))
                } else {
                    println!("unknown message");
                    self.out.close(CloseCode::Invalid)
                }
            }
        }
    }

    fn on_close(&mut self, _code: CloseCode, _reason: &str) {
        println!("closed");
        let mut lobbies = self.lobbies.borrow_mut();
        if let Some((lobby_id, player_id)) = self.connected_lobby_player {
            if let Some(lobby) = lobbies.get_mut(&lobby_id) {
                if let Some(player) = lobby.players.get_mut(&player_id) {
                    player.connection = Either::Right(Instant::now());
                }
            }
        }
    }

    fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
        println!("connected");
        Ok(())
    }
}

fn main() {
    // @Performance we could make a *mut pointer w/ unsafe
    let lobbies = Rc::new(RefCell::new(HashMap::new()));
    ws::listen("0.0.0.0:3012", |out| Server {
        out: out,
        lobbies: lobbies.clone(),
        connected_lobby_player: None,
    }).unwrap()
}
