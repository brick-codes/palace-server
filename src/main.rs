#![feature(vec_remove_item)]

#[macro_use]
extern crate log;
extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

mod game;

use game::GameState;
use rand::Rng;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
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

enum Connection {
   Connected(ws::Sender),
   Disconnected(Instant),
}

struct Player {
   name: String,
   connection: Connection,
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
   GameInProgress,
   EmptyPlayerName,
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
   GameInProgress,
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
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
}

#[derive(Serialize)]
enum ChooseFaceupError {
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
}

#[derive(Serialize)]
enum ReconnectError {
   LobbyNotFound,
   PlayerNotFound,
}

#[derive(Serialize)]
struct PlayerJoinEvent {
   num_players: u8,
}

#[derive(Serialize)]
enum PalaceOutMessage<'a> {
   NewLobbyResponse(&'a Result<NewLobbyResponse, NewLobbyError>),
   JoinLobbyResponse(&'a Result<JoinLobbyResponse, JoinLobbyError>),
   ListLobbiesResponse(&'a [LobbyDisplay<'a>]),
   StartGameResponse(&'a Result<(), StartGameError>),
   ChooseFaceupResponse(&'a Result<HandResponse<'a>, ChooseFaceupError>),
   MakePlayResponse(&'a Result<HandResponse<'a>, MakePlayError>),
   ReconnectResponse(&'a Result<(), ReconnectError>),
   PublicGameStateEvent(&'a game::PublicGameState<'a>),
   GameStartedEvent(&'a GameStartedEvent<'a>),
   PlayerJoinEvent(&'a PlayerJoinEvent),
}

struct Server {
   out: Sender,
   lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>>,
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
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some((lobby_id, player_id)) = self.connected_lobby_player {
         if let Some(lobby) = lobbies.get_mut(&lobby_id) {
            if let Some(player) = lobby.players.get_mut(&player_id) {
               player.connection = Connection::Disconnected(Instant::now());
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
               return Ok(());
            }

            if message.lobby_name.is_empty() {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Err(NewLobbyError::EmptyLobbyName)))?,
               )?;
               return Ok(());
            }

            if message.player_name.is_empty() {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::NewLobbyResponse(&Err(
                     NewLobbyError::EmptyPlayerName,
                  )))?,
               )?;
               return Ok(());
            }

            let mut lobbies = self.lobbies.write().unwrap();
            let lobby_id = LobbyId(rand::random());
            let player_id = PlayerId(rand::random());
            let mut players = HashMap::new();
            players.insert(
               player_id,
               Player {
                  name: message.player_name,
                  connection: Connection::Connected(self.out.clone()),
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
            if message.player_name.is_empty() {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(
                     JoinLobbyError::EmptyPlayerName,
                  )))?,
               )?;
               return Ok(());
            }
            let mut lobbies = self.lobbies.write().unwrap();
            let mut lobby_opt = lobbies.get_mut(&message.lobby_id);
            if let Some(lobby) = lobby_opt {
               if lobby.game.is_some() {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::JoinLobbyResponse(&Err(
                        JoinLobbyError::GameInProgress,
                     )))?,
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
                     connection: Connection::Connected(self.out.clone()),
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
               let new_num_players = lobby.players.len() as u8;
               for (id, player) in &mut lobby.players {
                  if *id == player_id {
                     continue;
                  }
                  match player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = send_or_log_and_report_ise(
                           sender,
                           serde_json::to_vec(&PalaceOutMessage::PlayerJoinEvent(&PlayerJoinEvent {
                              num_players: new_num_players,
                           }))?,
                        );
                     }
                     Connection::Disconnected(_) => (),
                  }
               }
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
            let lobbies = self.lobbies.read().unwrap();
            // @Performance we should be able to serialize with Serializer::collect_seq
            // and avoid collecting into a vector
            send_or_log_and_report_ise(
               &mut self.out,
               serde_json::to_vec(&PalaceOutMessage::ListLobbiesResponse(
                  &lobbies.iter().map(|(k, v)| v.display(k)).collect::<Vec<_>>(),
               ))?,
            )?;
            Ok(())
         }
         PalaceMessage::StartGame(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if lobby.game.is_some() {
                  self
                     .out
                     .send(serde_json::to_vec(&PalaceOutMessage::StartGameResponse(&Err(
                        StartGameError::GameInProgress,
                     )))?)?;
                  return Ok(());
               }

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
               let public_gs_json = serde_json::to_vec(&PalaceOutMessage::PublicGameStateEvent(&gs.public_state()))?;
               lobby.game = Some(gs);
               let mut turn_numbers: Vec<u8> = (0..num_players).collect();
               rand::thread_rng().shuffle(&mut turn_numbers);
               let mut turn_numbers = turn_numbers.into_iter();
               for player in lobby.players.values_mut() {
                  player.turn_number = turn_numbers.next().unwrap();
                  match player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = send_or_log_and_report_ise(
                           sender,
                           serde_json::to_vec(&PalaceOutMessage::GameStartedEvent(&GameStartedEvent {
                              hand: lobby.game.as_mut().unwrap().get_hand(player.turn_number),
                              turn_number: player.turn_number,
                           }))?,
                        );
                        let _ = send_or_log_and_report_ise(sender, public_gs_json.clone());
                     }
                     Connection::Disconnected(_) => (),
                  }
               }
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::StartGameResponse(&Ok(())))?,
               )?;
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
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        send_or_log_and_report_ise(
                           &mut self.out,
                           serde_json::to_vec(&PalaceOutMessage::ChooseFaceupResponse(&Err(
                              ChooseFaceupError::NotYourTurn,
                           )))?,
                        )?;
                        return Ok(());
                     }
                     gs.choose_three_faceup(message.card_one, message.card_two, message.card_three)
                  } else {
                     send_or_log_and_report_ise(
                        &mut self.out,
                        serde_json::to_vec(&PalaceOutMessage::ChooseFaceupResponse(&Err(
                           ChooseFaceupError::PlayerNotFound,
                        )))?,
                     )?;
                     return Ok(());
                  };

                  if result.is_ok() {
                     let public_gs_json =
                        serde_json::to_vec(&PalaceOutMessage::PublicGameStateEvent(&gs.public_state())).unwrap();
                     for (id, player) in &mut lobby.players {
                        match player.connection {
                           Connection::Connected(ref mut sender) => {
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
                           Connection::Disconnected(_) => (),
                        }
                     }
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("illegal")?)?;
                  }
                  Ok(())
               } else {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::ChooseFaceupResponse(&Err(
                        ChooseFaceupError::GameNotStarted,
                     )))?,
                  )?;
                  Ok(())
               }
            } else {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::ChooseFaceupResponse(&Err(
                     ChooseFaceupError::LobbyNotFound,
                  )))?,
               )?;
               Ok(())
            }
         }
         PalaceMessage::MakePlay(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        send_or_log_and_report_ise(
                           &mut self.out,
                           serde_json::to_vec(&PalaceOutMessage::MakePlayResponse(&Err(MakePlayError::NotYourTurn)))?,
                        )?;
                        return Ok(());
                     }
                     gs.make_play(message.cards)
                  } else {
                     send_or_log_and_report_ise(
                        &mut self.out,
                        serde_json::to_vec(&PalaceOutMessage::MakePlayResponse(&Err(MakePlayError::PlayerNotFound)))?,
                     )?;
                     return Ok(());
                  };

                  if result.is_ok() {
                     let public_gs_json =
                        serde_json::to_vec(&PalaceOutMessage::PublicGameStateEvent(&gs.public_state())).unwrap();
                     for (id, player) in &mut lobby.players {
                        match player.connection {
                           Connection::Connected(ref mut sender) => {
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
                           Connection::Disconnected(_) => (),
                        }
                     }
                  } else {
                     send_or_log_and_report_ise(&mut self.out, serde_json::to_vec("illegal")?)?;
                  }

                  Ok(())
               } else {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::MakePlayResponse(&Err(MakePlayError::GameNotStarted)))?,
                  )?;
                  Ok(())
               }
            } else {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::MakePlayResponse(&Err(MakePlayError::LobbyNotFound)))?,
               )?;
               Ok(())
            }
         }
         PalaceMessage::Reconnect(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(player) = lobby.players.get_mut(&message.player_id) {
                  player.connection = Connection::Connected(self.out.clone());
                  if let Some(ref gs) = lobby.game {
                     send_or_log_and_report_ise(
                        &mut self.out,
                        serde_json::to_vec(&PalaceOutMessage::GameStartedEvent(&GameStartedEvent {
                           hand: gs.get_hand(player.turn_number),
                           turn_number: player.turn_number,
                        }))?,
                     )?;
                     send_or_log_and_report_ise(
                        &mut self.out,
                        serde_json::to_vec(&PalaceOutMessage::PublicGameStateEvent(&gs.public_state()))?,
                     )?;
                  }
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::ReconnectResponse(&Ok(())))?,
                  )?;
                  Ok(())
               } else {
                  send_or_log_and_report_ise(
                     &mut self.out,
                     serde_json::to_vec(&PalaceOutMessage::ReconnectResponse(&Err(
                        ReconnectError::PlayerNotFound,
                     )))?,
                  )?;
                  Ok(())
               }
            } else {
               send_or_log_and_report_ise(
                  &mut self.out,
                  serde_json::to_vec(&PalaceOutMessage::ReconnectResponse(&Err(
                     ReconnectError::LobbyNotFound,
                  )))?,
               )?;
               Ok(())
            }
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
   // @Performance this could be a concurrent hashmap
   let lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>> = Arc::new(RwLock::new(HashMap::new()));

   // Spawn thread to clean up empty lobbies
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(std::time::Duration::from_secs(30));
         let mut lobbies = thread_lobbies.write().unwrap();
         lobbies.retain(|_, lobby| {
            for player in lobby.players.values() {
               match player.connection {
                  Connection::Connected(_) => {
                     return true;
                  }
                  Connection::Disconnected(_) => (),
               }
            }
            false
         });
      });
   }

   ws::listen("0.0.0.0:3012", |out| Server {
      out,
      lobbies: lobbies.clone(),
      connected_lobby_player: None,
   }).unwrap()
}

mod test {
   #[cfg(test)]
   use super::*;

   #[cfg(test)]
   use std::sync::{mpsc, Mutex};

   #[cfg(test)]
   #[derive(Deserialize)]
   struct TestLobbyDisplay {
      cur_players: u8,
      max_players: u8,
      started: bool,
      has_password: bool,
      owner: String,
      name: String,
      age: u64,
      lobby_id: String,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   struct TestNewLobbyResponse {
      player_id: PlayerId,
      lobby_id: LobbyId,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   enum TestNewLobbyError {
      LessThanTwoMaxPlayers,
      EmptyLobbyName,
      EmptyPlayerName,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   enum TestInMessage {
      ListLobbiesResponse(Box<[TestLobbyDisplay]>),
      NewLobbyResponse(Result<TestNewLobbyResponse, TestNewLobbyError>),
   }

   #[cfg(test)]
   #[derive(Serialize)]
   struct TestNewLobbyMessage {
      max_players: u8,
      password: String,
      lobby_name: String,
      player_name: String,
   }

   #[cfg(test)]
   #[derive(Serialize)]
   enum TestOutMessage {
      NewLobby(TestNewLobbyMessage),
      ListLobbies,
   }

   #[cfg(test)]
   struct TestClientInner {
      out: Sender,
      recvd_messages: mpsc::Sender<TestInMessage>,
      to_send_messages: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
   }

   #[cfg(test)]
   impl Handler for TestClientInner {
      fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
         self.out.timeout(100, ws::util::Token(1))
      }

      fn on_timeout(&mut self, event: ws::util::Token) -> ws::Result<()> {
         if event == ws::util::Token(1) {
            if let Ok(bytes) = self.to_send_messages.lock().unwrap().try_recv() {
               self.out.send(bytes).unwrap();
            }
            self.out.timeout(100, ws::util::Token(1))
         } else {
            Ok(())
         }
      }

      fn on_message(&mut self, msg: Message) -> ws::Result<()> {
         self
            .recvd_messages
            .send(serde_json::from_slice(&msg.into_data()).unwrap())
            .unwrap();
         Ok(())
      }
   }

   #[cfg(test)]
   struct TestClient {
      recvd_messages: mpsc::Receiver<TestInMessage>,
      to_send_messages: mpsc::Sender<Vec<u8>>,
   }

   #[cfg(test)]
   impl TestClient {
      fn new() -> TestClient {
         let (tx, rx) = mpsc::channel();
         let (tx2, rx2) = mpsc::channel();
         let to_send_messages = Arc::new(Mutex::new(rx2));
         std::thread::spawn(move || {
            ws::connect("ws://127.0.0.1:3012", |out| TestClientInner {
               out,
               recvd_messages: tx.clone(),
               to_send_messages: to_send_messages.clone(),
            }).unwrap();
         });
         TestClient {
            recvd_messages: rx,
            to_send_messages: tx2,
         }
      }

      fn send(&mut self, message: TestOutMessage) {
         self
            .to_send_messages
            .send(serde_json::to_vec(&message).unwrap())
            .unwrap();
      }

      fn get(&mut self) -> TestInMessage {
         self.recvd_messages.recv().unwrap()
      }

      fn disconnect(&mut self) {
         self
            .to_send_messages
            .send(Vec::from(
               "This message will be unrecognized, causing the connection to end",
            ))
            .unwrap();
      }
   }

   #[test]
   fn test_lobbies_clean_up() {
      // TODO: we should not call main, but have main and test call a spawn fn
      // that lets address be customizable so we can spawn a localhost only server
      std::thread::spawn(move || {
         main();
      });

      // Wait for server to be ready
      // TODO: could do this better with a retry loop or something
      std::thread::sleep(std::time::Duration::from_secs(5));

      // Create a lobby
      {
         let mut tc = TestClient::new();
         tc.send(TestOutMessage::NewLobby(TestNewLobbyMessage {
            player_name: "TestClient".into(),
            lobby_name: "TestLobby".into(),
            password: "".into(),
            max_players: 4,
         }));
         let nlr = tc.get();
         match nlr {
            TestInMessage::NewLobbyResponse(r) => assert!(r.is_ok()),
            _ => assert!(false),
         }
         tc.disconnect();
      }

      std::thread::sleep(std::time::Duration::from_secs(30));

      // Ensure lobby is cleaned up
      {
         let mut tc = TestClient::new();
         tc.send(TestOutMessage::ListLobbies);
         let llr = tc.get();
         match llr {
            TestInMessage::ListLobbiesResponse(r) => {
               assert!(r.is_empty());
            }
            _ => assert!(false),
         }
         tc.disconnect(); // TEMP see below
      }

      // Allow cleanup TEMP once we change prinln to log we won't need this
      std::thread::sleep(std::time::Duration::from_secs(5))
   }
}
