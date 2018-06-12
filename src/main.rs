#![feature(vec_remove_item)]

extern crate either;
#[macro_use]
extern crate log;
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
   fn display(&self, lobby_id: &LobbyId) -> LobbyDisplay {
      LobbyDisplay {
         cur_players: self.players.len() as u8,
         max_players: self.max_players,
         started: self.game.is_some(),
         has_password: !self.password.is_empty(),
         owner: &self.players[&self.owner].name,
         name: &self.name,
         age: self.creation_time.elapsed().as_secs(),
         lobby_id: *lobby_id,
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
   lobby_id: LobbyId,
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

#[derive(Serialize)]
enum NewLobbyError {
   LessThanTwoMaxPlayers,
   EmptyLobbyName,
   EmptyPlayerName,
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

#[derive(Serialize)]
enum JoinLobbyError {
   LobbyNotFound,
   LobbyFull,
   BadPassword,
   GameStarted,
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
struct MakePlayMessage {
   cards: Box<[game::Card]>,
   lobby_id: LobbyId,
   player_id: PlayerId,
}

#[derive(Serialize)]
struct HandResponse<'a> {
   hand: &'a [game::Card],
}

#[derive(Serialize)]
struct GameStartedEvent<'a> {
   hand: &'a [game::Card],
   turn_number: u8,
}

#[derive(Serialize)]
enum StartGameError {
   LobbyNotFound,
   NotLobbyOwner,
   LessThanTwoPlayers,
}

#[derive(Deserialize)]
struct ReconnectMessage {
   player_id: PlayerId,
   lobby_id: LobbyId,
}

#[derive(Deserialize)]
enum PalaceMessage {
   NewLobby(NewLobbyMessage),
   JoinLobby(JoinLobbyMessage),
   ListLobbies,
   StartGame(StartGameMessage),
   ChooseFaceup(ChooseFaceupMessage),
   MakePlay(MakePlayMessage),
   Reconnect(ReconnectMessage),
}

#[derive(Serialize)]
enum MakePlayError {

}

#[derive(Serialize)]
enum ChooseFaceupError {

}

#[derive(Serialize)]
enum PalaceOutMessage<'a> {
   NewLobbyResponse(&'a Result<NewLobbyResponse, NewLobbyError>),
   JoinLobbyResponse(&'a Result<JoinLobbyResponse, JoinLobbyError>),
   LobbyList(&'a [LobbyDisplay<'a>]),
   PublicGameState(&'a game::PublicGameState<'a>),
   ChooseFaceupResponse(&'a Result<HandResponse<'a>, ChooseFaceupError>),
   MakePlayResponse(&'a Result<HandResponse<'a>, MakePlayError>),
   StartGameResponse(&'a Result<(), StartGameError>),
   GameStarted(&'a GameStartedEvent<'a>),
}

struct Server {
   out: Sender,
   lobbies: Rc<RefCell<HashMap<LobbyId, Lobby>>>,
   // TODO: investigate a global hashmap of Sender(id?) -> (LobbyId, PlayerId) for this instead of storing this per socket
   connected_lobby_player: Option<(LobbyId, PlayerId)>,
}

enum OnMessageError {
   WebsocketError(ws::Error),
   SerdeError(serde_json::error::Error),
}

impl From<ws::Error> for OnMessageError {
   fn from(e: ws::Error) -> OnMessageError {
      OnMessageError::WebsocketError(e)
   }
}

impl From<serde_json::error::Error> for OnMessageError {
   fn from(e: serde_json::error::Error) -> OnMessageError {
      OnMessageError::SerdeError(e)
   }
}

impl Handler for Server {
   fn on_message(&mut self, msg: Message) -> ws::Result<()> {
      println!("got message");
      match msg {
         Message::Text(_) => self.out.close(CloseCode::Unsupported),
         Message::Binary(binary) => {
            unsafe {
               println!("DEBUG MESSAGE: {}", String::from_utf8_unchecked(binary.clone()));
            }
            let message: Result<PalaceMessage, serde_json::error::Error> = serde_json::from_slice(&binary);
            if let Err(e) = message {
               println!("ERROR DECODING MESSAGE: {:?}", e);
            }
            if let Ok(message) = serde_json::from_slice::<PalaceMessage>(&binary) {
               match self.handle_message(message) {
                  Ok(()) => Ok(()),
                  // We don't log an error here because that is done
                  // in `send_or_log_and_report_ise`
                  // an error here would just be an error sending
                  // ISE which we can't handle sanely
                  Err(OnMessageError::WebsocketError(e)) => Err(e),
                  Err(OnMessageError::SerdeError(e)) => {
                     error!("Failed to serialize a message: {:?}", e);
                     self.out.send(ws::Message::binary("\"InternalServerError\""))
                  }
               }
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

impl Server {
   fn handle_message(&mut self, message: PalaceMessage) -> Result<(), OnMessageError> {
      match message {
         PalaceMessage::NewLobby(message) => {
            if message.max_players < 2 {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Err(
                     NewLobbyError::LessThanTwoMaxPlayers,
                  )))?,
               )?;
            }
            if message.lobby_name.is_empty() {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Err(
                     NewLobbyError::EmptyLobbyName,
                  )))?,
               )?;
            }
            if message.player_name.is_empty() {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Err(
                     NewLobbyError::EmptyPlayerName,
                  )))?,
               )?;
            }
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
                  players,
                  game: None,
                  password: message.password,
                  name: message.lobby_name,
                  owner: player_id,
                  max_players: message.max_players,
                  creation_time: Instant::now(),
               },
            );
            self.connected_lobby_player = Some((lobby_id, player_id));
            send_or_log_and_report_ise(
               &mut self.out,
               serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Ok(NewLobbyResponse {
                  player_id,
                  lobby_id,
               })))?,
            )?;
            Ok(())
         }
         PalaceMessage::JoinLobby(message) => {
            // TODO (APPLIES TO ALL)
            // PUT THIS IN A FN THAT RETURNS A RESULT<JoinLobbyResponse, JoinLobbyErr)
            // DO THE JSON SERIALIZATION / MESSAGE SENDING AT THE TOP LEVEL IN ONE PLACE
            let mut lobbies = self.lobbies.borrow_mut();
            let mut lobby_opt = lobbies.get_mut(&message.lobby_id);
            if let Some(lobby) = lobby_opt {
               if lobby.game.is_some() {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(JoinLobbyError::GameStarted)))?,
                  )?;
                  return Ok(());
               }

               if lobby.password != message.password {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(JoinLobbyError::BadPassword)))?,
                  )?;
                  return Ok(());
               }

               if lobby.players.len() as u8 >= lobby.max_players {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(JoinLobbyError::LobbyFull)))?,
                  )?;
                  return Ok(());
               }

               let player_id = PlayerId(rand::random());
               lobby.players.insert(
                  player_id,
                  Player {
                     name: message.player_name,
                     connection: Either::Left(self.out.clone()),
                     turn_number: 0,
                  },
               );
               self.connected_lobby_player = Some((message.lobby_id, player_id));
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Ok(JoinLobbyResponse {
                     player_id,
                  })))?,
               )?;
               Ok(())
            } else {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(
                     JoinLobbyError::LobbyNotFound,
                  )))?,
               )?;
               Ok(())
            }
         }
         PalaceMessage::ListLobbies => {
            println!("got lobby message");
            let mut lobbies = self.lobbies.borrow_mut();
            // @Performance we should be able to serialize with Serializer::collect_seq
            // and avoid collecting into a vector
            send_or_log_and_report_ise(
               &mut self.out,
               serde_json::to_vec(&PalaceOutMessage::LobbyList(
                  &lobbies.iter().map(|(k, v)| v.display(k)).collect::<Vec<_>>(),
               ))?,
            )?;
            Ok(())
         }
         PalaceMessage::StartGame(message) => {
            let mut lobbies = self.lobbies.borrow_mut();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if message.player_id != lobby.owner {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::StartGameResponse(&Err(
                        StartGameError::NotLobbyOwner,
                     )))?,
                  )?;
                  return Ok(());
               }

               if lobby.players.len() < 2 {
                  self
                     .out
                     .send(serde_json::to_vec(&PalaceOutMessage::StartGameResponse(&Err(
                        StartGameError::LessThanTwoPlayers,
                     )))?)?;
                  return Ok(());
               }

               let num_players = lobby.players.len() as u8;
               let gs = GameState::new(num_players);
               let public_gs_json = serde_json::to_vec(&PalaceOutMessage::PublicGameState(&gs.public_state()))?;
               lobby.game = Some(gs);
               let mut turn_numbers: Vec<u8> = (0..num_players).collect();
               rand::thread_rng().shuffle(&mut turn_numbers);
               let mut turn_numbers = turn_numbers.into_iter();
               for player in lobby.players.values_mut() {
                  player.turn_number = turn_numbers.next().unwrap();
                  match player.connection {
                     either::Left(ref mut sender) => {
                        let _ = send_or_log_and_report_ise(sender, public_gs_json.clone());
                        match lobby.game {
                           Some(ref mut gs) => {
                              let _ = send_or_log_and_report_ise(
                                 sender,
                                 serde_json::to_vec(&PalaceOutMessage::GameStarted(&GameStartedEvent {
                                    hand: gs.get_hand(player.turn_number),
                                    turn_number: player.turn_number,
                                 }))?,
                              );
                           }
                           None => unreachable!(),
                        }
                     }
                     either::Right(_) => (),
                  }
               }
               Ok(())
            } else {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::StartGameResponse(&Err(
                     StartGameError::LobbyNotFound,
                  )))?,
               )?;
               Ok(())
            }
         }
         PalaceMessage::ChooseFaceup(message) => {
            let mut lobbies = self.lobbies.borrow_mut();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("it is not your turn")?)?;
                        return Ok(());
                     }
                     gs.choose_three_faceup(message.card_one, message.card_two, message.card_three)
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("player does not exist")?)?;
                     return Ok(());
                  };

                  if result.is_ok() {
                     let public_gs_json =
                        serde_json::to_vec(&PalaceOutMessage::PublicGameState(&gs.public_state())).unwrap();
                     for (id, player) in lobby.players.iter_mut() {
                        match player.connection {
                           either::Left(ref mut sender) => {
                              let _ = send_or_log_and_report_ise(sender, public_gs_json.clone());
                              if *id == message.player_id {
                                 let _ = send_or_log_and_report_ise(
                                    sender,
                                    serde_json::to_vec(&PalaceOutMessage::ChooseFaceupResponse(&Ok(HandResponse {
                                       hand: gs.get_hand(player.turn_number),
                                    })))?,
                                 );
                              }
                           }
                           either::Right(_) => (),
                        }
                     }
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("illegal")?)?;
                  }
                  Ok(())
               } else {
                  send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("game has not started")?)?;
                  Ok(())
               }
            } else {
               send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("lobby does not exist")?)?;
               Ok(())
            }
         }
         PalaceMessage::MakePlay(message) => {
            let mut lobbies = self.lobbies.borrow_mut();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("it is not your turn")?)?;
                        return Ok(());
                     }
                     gs.make_play(message.cards)
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("player does not exist")?)?;
                     return Ok(());
                  };

                  if result.is_ok() {
                     let public_gs_json =
                        serde_json::to_vec(&PalaceOutMessage::PublicGameState(&gs.public_state())).unwrap();
                     for (id, player) in lobby.players.iter_mut() {
                        match player.connection {
                           either::Left(ref mut sender) => {
                              let _ = send_or_log_and_report_ise(sender, public_gs_json.clone());
                              if *id == message.player_id {
                                 let _ = send_or_log_and_report_ise(
                                    sender,
                                    serde_json::to_vec(&PalaceOutMessage::MakePlayResponse(&Ok(HandResponse {
                                       hand: gs.get_hand(player.turn_number),
                                    })))?,
                                 );
                              }
                           }
                           either::Right(_) => (),
                        }
                     }
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("illegal")?)?;
                  }

                  Ok(())
               } else {
                  send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("game has not started")?)?;
                  Ok(())
               }
            } else {
               send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("lobby does not exist")?)?;
               Ok(())
            }
         }
         PalaceMessage::Reconnect(message) => {
            /*
            let mut lobbies = self.lobbies.borrow_mut();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(player) = lobbies.get_mut(&message.player_id) {
                  if let Some(ref gs)
               } else {

               }
            } else {

            } */
            Ok(())
         }
      }
   }
}

fn send_or_log_and_report_ise(s: &mut Sender, message: Vec<u8>) -> ws::Result<()> {
   if let Err(e) = s.send(message) {
      error!("Failed to send a message: {:?}", e);
      s.send(ws::Message::binary("\"InternalServerError\""))
   } else {
      Ok(())
   }
}

fn main() {
   // @Performance we could make a *mut pointer w/ unsafe
   let lobbies = Rc::new(RefCell::new(HashMap::new()));
   ws::listen("0.0.0.0:3012", |out| Server {
      out,
      lobbies: lobbies.clone(),
      connected_lobby_player: None,
   }).unwrap()
}
