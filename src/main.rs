#![feature(vec_remove_item)]

extern crate pretty_env_logger;
#[macro_use]
extern crate log;
extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

mod ai;
mod data;
mod game;

use ai::PalaceAi;
use data::*;
use game::GameState;
use rand::Rng;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::HashMap;
use std::collections::HashSet;
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
   ai_players: HashSet<u8>,
   max_players: u8,
   password: String,
   game: Option<GameState>,
   owner: PlayerId,
   name: String,
   creation_time: Instant,
}

enum Connection {
   Connected(ws::Sender),
   Disconnected(Instant),
   Ai(Box<PalaceAi + Send + Sync>),
}

struct Player {
   name: String,
   connection: Connection,
   turn_number: u8,
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
      let recv_time = Instant::now();
      let result = match msg {
         Message::Text(_) => {
            debug!("Received text; closing connection");
            self.out.close(CloseCode::Unsupported)
         }
         Message::Binary(binary) => {
            debug!(
               "Received bytes as string: {}",
               String::from_utf8(binary.clone()).unwrap_or_else(|_| "[invalid utf-8]".into())
            );
            match serde_json::from_slice::<PalaceInMessage>(&binary) {
               Ok(message) => {
                  // We don't log an error here because that is done
                  // in `serialize_and_send`
                  // an error here would just be an error sending
                  // ISE which we can't handle sanely
                  self.handle_message(message)
               }
               Err(e) => {
                  debug!(
                     "Received a binary message but could not decode it into an object; Error: {:?}",
                     e
                  );
                  self.out.close(CloseCode::Invalid)
               }
            }
         }
      };
      trace!("Response time: {:?}", recv_time.elapsed());
      result
   }

   fn on_close(&mut self, _code: CloseCode, _reason: &str) {
      debug!("A connection closed");
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
      debug!("A connection opened");
      Ok(())
   }
}

impl Server {
   fn handle_message(&mut self, message: PalaceInMessage) -> ws::Result<()> {
      match message {
         PalaceInMessage::NewLobby(message) => {
            if message.max_players < 2 {
               return serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::NewLobbyResponse(Err(NewLobbyError::LessThanTwoMaxPlayers)),
               );
            }

            if message.lobby_name.is_empty() {
               return serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::NewLobbyResponse(Err(NewLobbyError::EmptyLobbyName)),
               );
            }

            if message.player_name.is_empty() {
               return serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::NewLobbyResponse(Err(NewLobbyError::EmptyPlayerName)),
               );
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
                  ai_players: HashSet::new(),
                  game: None,
                  password: message.password,
                  name: message.lobby_name,
                  owner: player_id,
                  max_players: message.max_players,
                  creation_time: Instant::now(),
               },
            );
            self.connected_lobby_player = Some((lobby_id, player_id));
            serialize_and_send(
               &mut self.out,
               &PalaceOutMessage::NewLobbyResponse(Ok(NewLobbyResponse { player_id, lobby_id })),
            )
         }
         PalaceInMessage::JoinLobby(message) => {
            // TODO (APPLIES TO ALL)
            // PUT THIS IN A FN THAT RETURNS A RESULT<JoinLobbyResponse, JoinLobbyErr)
            // DO THE JSON SERIALIZATION / MESSAGE SENDING AT THE TOP LEVEL IN ONE PLACE
            if message.player_name.is_empty() {
               return serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::JoinLobbyResponse(Err(JoinLobbyError::EmptyPlayerName)),
               );
            }
            let mut lobbies = self.lobbies.write().unwrap();
            let mut lobby_opt = lobbies.get_mut(&message.lobby_id);
            if let Some(lobby) = lobby_opt {
               if lobby.game.is_some() {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::JoinLobbyResponse(Err(JoinLobbyError::GameInProgress)),
                  );
               }

               if lobby.password != message.password {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::JoinLobbyResponse(Err(JoinLobbyError::BadPassword)),
                  );
               }

               if lobby.players.len() as u8 >= lobby.max_players {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::JoinLobbyResponse(Err(JoinLobbyError::LobbyFull)),
                  );
               }

               let player_id = PlayerId(rand::random());
               serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::JoinLobbyResponse(Ok(JoinLobbyResponse {
                     player_id,
                     lobby_players: lobby.players.values().map(|x| x.name.as_ref()).collect(),
                  })),
               )?;

               lobby.players.insert(
                  player_id,
                  Player {
                     name: message.player_name,
                     connection: Connection::Connected(self.out.clone()),
                     turn_number: 0,
                  },
               );
               self.connected_lobby_player = Some((message.lobby_id, player_id));

               let new_num_players = lobby.players.len() as u8;
               for (id, player) in &mut lobby.players {
                  if *id == player_id {
                     continue;
                  }
                  match player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = serialize_and_send(
                           sender,
                           &PalaceOutMessage::PlayerJoinEvent(PlayerJoinEvent {
                              total_num_players: new_num_players,
                              new_player_name: &player.name,
                           }),
                        );
                     }
                     Connection::Disconnected(_) => (),
                     Connection::Ai(_) => unreachable!(),
                  }
               }
               Ok(())
            } else {
               return serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::JoinLobbyResponse(Err(JoinLobbyError::LobbyNotFound)),
               );
            }
         }
         PalaceInMessage::ListLobbies => {
            let lobbies = self.lobbies.read().unwrap();
            // @Performance we should be able to serialize with Serializer::collect_seq
            // and avoid collecting into a vector
            serialize_and_send(
               &mut self.out,
               &PalaceOutMessage::ListLobbiesResponse(&lobbies.iter().map(|(k, v)| v.display(k)).collect::<Vec<_>>()),
            )
         }
         PalaceInMessage::StartGame(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if lobby.game.is_some() {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::StartGameResponse(Err(StartGameError::GameInProgress)),
                  );
               }

               if message.player_id != lobby.owner {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::StartGameResponse(Err(StartGameError::NotLobbyOwner)),
                  );
               }

               if lobby.players.len() < 2 {
                  return serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::StartGameResponse(Err(StartGameError::LessThanTwoPlayers)),
                  );
               }

               let num_players = lobby.players.len() as u8;
               let gs = GameState::new(num_players);
               lobby.game = Some(gs);

               let public_gs = lobby.game.as_ref().unwrap().public_state();
               let mut turn_numbers: Vec<u8> = (0..num_players).collect();
               rand::thread_rng().shuffle(&mut turn_numbers);
               let mut turn_numbers = turn_numbers.into_iter();
               for player in lobby.players.values_mut() {
                  player.turn_number = turn_numbers.next().unwrap();
                  match player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = serialize_and_send(
                           sender,
                           &PalaceOutMessage::GameStartedEvent(GameStartedEvent {
                              hand: lobby.game.as_ref().unwrap().get_hand(player.turn_number),
                              turn_number: player.turn_number,
                           }),
                        );
                        let _ = serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
                     }
                     Connection::Disconnected(_) => (),
                     Connection::Ai(ref mut ai) => ai.on_game_start(GameStartedEvent {
                        hand: lobby.game.as_ref().unwrap().get_hand(player.turn_number),
                        turn_number: player.turn_number,
                     }),
                  }
               }

               serialize_and_send(&mut self.out, &PalaceOutMessage::StartGameResponse(Ok(())))
            } else {
               serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::StartGameResponse(Err(StartGameError::LobbyNotFound)),
               )
            }
         }
         PalaceInMessage::ChooseFaceup(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        return serialize_and_send(
                           &mut self.out,
                           &PalaceOutMessage::ChooseFaceupResponse(Err(ChooseFaceupError::NotYourTurn)),
                        );
                     }
                     gs.choose_three_faceup(message.card_one, message.card_two, message.card_three)
                  } else {
                     return serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::ChooseFaceupResponse(Err(ChooseFaceupError::PlayerNotFound)),
                     );
                  };

                  match result {
                     Ok(()) => {
                        let public_gs = gs.public_state();
                        for (id, player) in &mut lobby.players {
                           match player.connection {
                              Connection::Connected(ref mut sender) => {
                                 if *id == message.player_id {
                                    let _ = serialize_and_send(
                                       sender,
                                       &PalaceOutMessage::ChooseFaceupResponse(Ok(HandResponse {
                                          hand: gs.get_hand(player.turn_number),
                                       })),
                                    );
                                 }
                                 let _ =
                                    serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
                              }
                              Connection::Disconnected(_) => (),
                              Connection::Ai(ref mut ai) => {
                                 if *id == message.player_id {
                                    ai.on_hand_update(gs.get_hand(player.turn_number));
                                 }
                                 ai.on_game_state_update(&public_gs);
                              }
                           }
                        }

                        Ok(())
                     }
                     Err(e) => serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::ChooseFaceupResponse(Err(ChooseFaceupError::GameError(e))),
                     ),
                  }
               } else {
                  serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::ChooseFaceupResponse(Err(ChooseFaceupError::GameNotStarted)),
                  )
               }
            } else {
               serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::ChooseFaceupResponse(Err(ChooseFaceupError::LobbyNotFound)),
               )
            }
         }
         PalaceInMessage::MakePlay(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(ref mut gs) = lobby.game {
                  let result = if let Some(player) = lobby.players.get(&message.player_id) {
                     if player.turn_number != gs.active_player {
                        return serialize_and_send(
                           &mut self.out,
                           &PalaceOutMessage::MakePlayResponse(Err(MakePlayError::NotYourTurn)),
                        );
                     }
                     gs.make_play(message.cards)
                  } else {
                     return serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::MakePlayResponse(Err(MakePlayError::PlayerNotFound)),
                     );
                  };

                  match result {
                     Ok(()) => {
                        let public_gs = gs.public_state();
                        for (id, player) in &mut lobby.players {
                           match player.connection {
                              Connection::Connected(ref mut sender) => {
                                 if *id == message.player_id {
                                    let _ = serialize_and_send(
                                       sender,
                                       &PalaceOutMessage::MakePlayResponse(Ok(HandResponse {
                                          hand: gs.get_hand(player.turn_number),
                                       })),
                                    );
                                 }
                                 let _ =
                                    serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
                              }
                              Connection::Disconnected(_) => (),
                              Connection::Ai(ref mut ai) => {
                                 if *id == message.player_id {
                                    ai.on_hand_update(gs.get_hand(player.turn_number));
                                 }
                                 ai.on_game_state_update(&public_gs);
                              }
                           }
                        }

                        Ok(())
                     }
                     Err(e) => serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::MakePlayResponse(Err(MakePlayError::GameError(e))),
                     ),
                  }
               } else {
                  serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::MakePlayResponse(Err(MakePlayError::GameNotStarted)),
                  )
               }
            } else {
               serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::MakePlayResponse(Err(MakePlayError::LobbyNotFound)),
               )
            }
         }
         PalaceInMessage::Reconnect(message) => {
            let mut lobbies = self.lobbies.write().unwrap();
            if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
               if let Some(player) = lobby.players.get_mut(&message.player_id) {
                  player.connection = Connection::Connected(self.out.clone());
                  if let Some(ref gs) = lobby.game {
                     let _ = serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::GameStartedEvent(GameStartedEvent {
                           hand: gs.get_hand(player.turn_number),
                           turn_number: player.turn_number,
                        }),
                     );
                     let _ = serialize_and_send(
                        &mut self.out,
                        &PalaceOutMessage::PublicGameStateEvent(&gs.public_state()),
                     );
                  }

                  serialize_and_send(&mut self.out, &PalaceOutMessage::ReconnectResponse(Ok(())))
               } else {
                  serialize_and_send(
                     &mut self.out,
                     &PalaceOutMessage::ReconnectResponse(Err(ReconnectError::PlayerNotFound)),
                  )
               }
            } else {
               serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::ReconnectResponse(Err(ReconnectError::LobbyNotFound)),
               )?;
               Ok(())
            }
         }
      }
   }
}

fn serialize_and_send(s: &mut Sender, message: &PalaceOutMessage) -> ws::Result<()> {
   match serde_json::to_vec(message) {
      Ok(bytes) => {
         if let Err(e) = s.send(bytes) {
            error!("Failed to send a message: {:?}", e);
            s.send(ws::Message::binary("\"InternalServerError\""))
         } else {
            Ok(())
         }
      }
      Err(e) => {
         error!("Failed to serialize a message: {:?}", e);
         s.send(ws::Message::binary("\"InternalServerError\""))
      }
   }
}

fn run_server(address: &'static str) {
   pretty_env_logger::init();
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
                  Connection::Ai(_) => (),
               }
            }
            false
         });
      });
   }

   ws::listen(address, |out| Server {
      out,
      lobbies: lobbies.clone(),
      connected_lobby_player: None,
   }).unwrap()
}

fn main() {
   run_server("0.0.0.0:3012")
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
            ws::connect("ws://127.0.0.1:3013", |out| TestClientInner {
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
      std::thread::spawn(move || {
         run_server("127.0.0.1:3013");
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

      // TEMP why do we need this?? test passes but annoying error after the test run
      std::thread::sleep(std::time::Duration::from_secs(5))
   }
}
