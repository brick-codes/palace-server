#![feature(vec_remove_item, nll)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate rand;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

#[cfg(test)]
extern crate parking_lot;

mod ai;
mod data;
mod game;

use ai::PalaceAi;
use data::*;
use game::GameState;
use rand::Rng;
use serde::{Deserialize, Deserializer, Serializer};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
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
   ai_players: HashMap<u8, PlayerId>,
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
   Ai(Box<dyn PalaceAi + Send + Sync>),
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

fn ai_play_loop(lobbies: &Arc<RwLock<HashMap<LobbyId, Lobby>>>) {
   loop {
      std::thread::sleep(Duration::from_millis(100));
      //let ai_loop_start = Instant::now();
      {
         let mut lobbies = lobbies.write().unwrap();
         for lobby in &mut lobbies.values_mut() {
            if let Some(ref mut gs) = lobby.game {
               if let Some(player_id) = lobby.ai_players.get(&gs.active_player) {
                  match gs.cur_phase {
                     game::GamePhase::Setup => {
                        let faceup_three = match lobby.players.get_mut(player_id).unwrap().connection {
                           Connection::Ai(ref mut ai) => ai.choose_three_faceup(),
                           _ => unreachable!(),
                        };
                        match gs.choose_three_faceup(faceup_three.0, faceup_three.1, faceup_three.2) {
                           Ok(()) => {
                              report_choose_faceup(&gs, &mut lobby.players, *player_id);
                           }
                           Err(_) => {
                              let player = lobby.players.get_mut(player_id).unwrap();
                              match player.connection {
                                 Connection::Ai(ref mut ai) => {
                                    error!("Bot (strategy: {}) failed to choose three faceup", ai.strategy_name());
                                    if ai.strategy_name() != "Random" {
                                       info!("Falling back to Random");
                                       *ai = Box::new(ai::random::new());
                                       ai.on_game_start(GameStartEvent {
                                          hand: gs.get_hand(player.turn_number),
                                          turn_number: player.turn_number,
                                          players: &HashMap::new(), // TODO: fill this
                                       });
                                       ai.on_game_state_update(&gs.public_state());
                                    }
                                 }
                                 _ => unreachable!(),
                              }
                           }
                        }
                     }
                     game::GamePhase::Play => {
                        let play = if gs.hands[gs.active_player as usize].is_empty()
                           && gs.face_up_three[gs.active_player as usize].is_empty()
                        {
                           vec![].into_boxed_slice()
                        } else {
                           match lobby.players.get_mut(player_id).unwrap().connection {
                              Connection::Ai(ref mut ai) => ai.take_turn(),
                              _ => unreachable!(),
                           }
                        };
                        match gs.make_play(play) {
                           Ok(()) => {
                              report_make_play(&gs, &mut lobby.players, *player_id);
                           }
                           Err(_) => {
                              let player = lobby.players.get_mut(player_id).unwrap();
                              match player.connection {
                                 Connection::Ai(ref mut ai) => {
                                    error!("Bot (strategy: {}) failed to make play", ai.strategy_name());
                                    if ai.strategy_name() != "Random" {
                                       info!("Falling back to Random");
                                       *ai = Box::new(ai::random::new());
                                       ai.on_game_start(GameStartEvent {
                                          hand: gs.get_hand(player.turn_number),
                                          turn_number: player.turn_number,
                                          players: &HashMap::new(), // TODO: fill this
                                       });
                                       ai.on_game_state_update(&gs.public_state());
                                    }
                                 }
                                 _ => unreachable!(),
                              }
                           }
                        }
                     }
                     game::GamePhase::Complete => {
                        continue;
                     }
                  }
               }
            } else {
               continue;
            }
         }
      }
      //trace!("AI runtime: {:?}", ai_loop_start.elapsed());
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
            debug!("Received bytes (as string): {}", String::from_utf8_lossy(&binary));
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
      disconnect_old_player(&mut self.connected_lobby_player, &mut lobbies);
   }

   fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
      debug!("A connection opened");
      Ok(())
   }
}

impl Server {
   fn handle_message(&mut self, message: PalaceInMessage) -> ws::Result<()> {
      match message {
         PalaceInMessage::RequestAi(message) => {
            let response = PalaceOutMessage::RequestAiResponse(self.do_request_ai(message));
            serialize_and_send(&mut self.out, &response)
         }
         PalaceInMessage::NewLobby(message) => {
            let response = PalaceOutMessage::NewLobbyResponse(self.do_new_lobby(message));
            serialize_and_send(&mut self.out, &response)
         }
         PalaceInMessage::JoinLobby(message) => {
            // This is an unfortunate special case,
            // we can't (efficiently) delay sending the join lobby response until the end of the message
            // so on the Ok path the JoinLobbyResponse has already been sent
            match self.do_join_lobby(message) {
               Ok(()) => Ok(()),
               Err(e) => serialize_and_send(&mut self.out, &PalaceOutMessage::JoinLobbyResponse(Err(e))),
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
            let response = PalaceOutMessage::StartGameResponse(self.do_start_game(message));
            serialize_and_send(&mut self.out, &response)
         }
         PalaceInMessage::ChooseFaceup(message) => {
            let response = PalaceOutMessage::ChooseFaceupResponse(self.do_choose_faceup(message));
            serialize_and_send(&mut self.out, &response)
         }
         PalaceInMessage::MakePlay(message) => {
            let response = PalaceOutMessage::MakePlayResponse(self.do_make_play(message));
            serialize_and_send(&mut self.out, &response)
         }
         PalaceInMessage::Reconnect(message) => {
            let response = PalaceOutMessage::ReconnectResponse(self.do_reconnect(&message));
            serialize_and_send(&mut self.out, &response)
         }
      }
   }

   fn do_request_ai(&mut self, message: RequestAiMessage) -> Result<(), RequestAiError> {
      if message.num_ai == 0 {
         return Err(RequestAiError::LessThanOneAiRequested);
      }

      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if lobby.owner != message.player_id {
            Err(RequestAiError::NotLobbyOwner)
         } else if lobby.players.len() + message.num_ai as usize > lobby.max_players as usize {
            Err(RequestAiError::LobbyTooSmall)
         } else {
            for _ in 0..message.num_ai {
               let player_id = PlayerId(rand::random());
               let ai: Box<PalaceAi + Send + Sync> = Box::new(ai::random::new());
               add_player(
                  Player {
                     name: ai::get_bot_name(),
                     connection: Connection::Ai(ai),
                     turn_number: 0,
                  },
                  player_id,
                  lobby,
               );
            }
            Ok(())
         }
      } else {
         Err(RequestAiError::LobbyNotFound)
      }
   }

   fn do_new_lobby(&mut self, message: NewLobbyMessage) -> Result<NewLobbyResponse, NewLobbyError> {
      if message.max_players < 2 {
         return Err(NewLobbyError::LessThanTwoMaxPlayers);
      }

      if message.lobby_name.is_empty() {
         return Err(NewLobbyError::EmptyLobbyName);
      }

      if message.player_name.is_empty() {
         return Err(NewLobbyError::EmptyPlayerName);
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
            ai_players: HashMap::new(),
            game: None,
            password: message.password,
            name: message.lobby_name,
            owner: player_id,
            max_players: message.max_players,
            creation_time: Instant::now(),
         },
      );

      update_connected_player_info(&mut self.connected_lobby_player, &mut lobbies, lobby_id, player_id);

      Ok(NewLobbyResponse {
         player_id,
         lobby_id,
         max_players: message.max_players,
      })
   }

   fn do_join_lobby(&mut self, message: JoinLobbyMessage) -> Result<(), JoinLobbyError> {
      if message.player_name.is_empty() {
         return Err(JoinLobbyError::EmptyPlayerName);
      }

      let mut lobbies = self.lobbies.write().unwrap();
      let new_player_id = if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if lobby.game.is_some() {
            return Err(JoinLobbyError::GameInProgress);
         }

         if lobby.password != message.password {
            return Err(JoinLobbyError::BadPassword);
         }

         if lobby.players.len() as u8 >= lobby.max_players {
            return Err(JoinLobbyError::LobbyFull);
         }

         let player_id = PlayerId(rand::random());

         let lobby_players = {
            let mut lobby_players: Vec<&str> = vec![lobby.players[&lobby.owner].name.as_ref()];
            lobby_players.extend(
               lobby
                  .players
                  .iter()
                  .filter(|(id, _)| **id != lobby.owner)
                  .map(|(_, p)| p.name.as_str()),
            );
            lobby_players
         };

         let _ = serialize_and_send(
            &mut self.out,
            &PalaceOutMessage::JoinLobbyResponse(Ok(JoinLobbyResponse {
               player_id,
               lobby_players,
               max_players: lobby.max_players,
            })),
         );

         add_player(
            Player {
               name: message.player_name,
               connection: Connection::Connected(self.out.clone()),
               turn_number: 0,
            },
            player_id,
            lobby,
         );

         player_id
      } else {
         return Err(JoinLobbyError::LobbyNotFound);
      };

      update_connected_player_info(
         &mut self.connected_lobby_player,
         &mut lobbies,
         message.lobby_id,
         new_player_id,
      );

      Ok(())
   }

   fn do_start_game(&mut self, message: StartGameMessage) -> Result<(), StartGameError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if lobby.game.is_some() {
            return Err(StartGameError::GameInProgress);
         }

         if message.player_id != lobby.owner {
            return Err(StartGameError::NotLobbyOwner);
         }

         if lobby.players.len() < 2 {
            return Err(StartGameError::LessThanTwoPlayers);
         }

         let num_players = lobby.players.len() as u8;
         let gs = GameState::new(num_players);
         lobby.game = Some(gs);

         let public_gs = lobby.game.as_ref().unwrap().public_state();

         let mut turn_numbers: Vec<u8> = (0..num_players).collect();
         rand::thread_rng().shuffle(&mut turn_numbers);
         let mut turn_numbers = turn_numbers.into_iter();

         let mut players = HashMap::new();
         // Assign everyone turn numbers
         for player in lobby.players.values_mut() {
            player.turn_number = turn_numbers.next().unwrap();
            // @Performance: we can avoid cloning here
            // because we don't modify the hashmap before we send the data.
            // the problem is convincing that to the rust compiler
            // which sees us mutably borrowing the hashmap
            // (to send data out). So, use unsafe?
            players.insert(player.turn_number, player.name.clone());
         }

         // Send out game start events
         for (id, player) in &mut lobby.players {
            match player.connection {
               Connection::Connected(ref mut sender) => {
                  let _ = serialize_and_send(
                     sender,
                     &PalaceOutMessage::GameStartEvent(GameStartEvent {
                        hand: lobby.game.as_ref().unwrap().get_hand(player.turn_number),
                        turn_number: player.turn_number,
                        players: &players,
                     }),
                  );
                  let _ = serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
               }
               Connection::Disconnected(_) => (),
               Connection::Ai(ref mut ai) => {
                  lobby.ai_players.insert(player.turn_number, *id);
                  ai.on_game_start(GameStartEvent {
                     hand: lobby.game.as_ref().unwrap().get_hand(player.turn_number),
                     turn_number: player.turn_number,
                     players: &players,
                  });
                  ai.on_game_state_update(&public_gs);
               }
            }
         }

         Ok(())
      } else {
         Err(StartGameError::LobbyNotFound)
      }
   }

   fn do_choose_faceup(&mut self, message: ChooseFaceupMessage) -> Result<(), ChooseFaceupError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if let Some(ref mut gs) = lobby.game {
            let result = if let Some(player) = lobby.players.get(&message.player_id) {
               if player.turn_number != gs.active_player {
                  return Err(ChooseFaceupError::NotYourTurn);
               }

               gs.choose_three_faceup(message.card_one, message.card_two, message.card_three)
            } else {
               return Err(ChooseFaceupError::PlayerNotFound);
            };

            match result {
               Ok(()) => {
                  report_choose_faceup(&gs, &mut lobby.players, message.player_id);
                  Ok(())
               }
               Err(e) => Err(ChooseFaceupError::GameError(e)),
            }
         } else {
            Err(ChooseFaceupError::GameNotStarted)
         }
      } else {
         Err(ChooseFaceupError::LobbyNotFound)
      }
   }

   fn do_make_play(&mut self, message: MakePlayMessage) -> Result<(), MakePlayError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if let Some(ref mut gs) = lobby.game {
            let result = if let Some(player) = lobby.players.get(&message.player_id) {
               if player.turn_number != gs.active_player {
                  return Err(MakePlayError::NotYourTurn);
               }

               gs.make_play(message.cards)
            } else {
               return Err(MakePlayError::PlayerNotFound);
            };

            match result {
               Ok(()) => {
                  report_make_play(&gs, &mut lobby.players, message.player_id);
                  Ok(())
               }
               Err(e) => Err(MakePlayError::GameError(e)),
            }
         } else {
            return Err(MakePlayError::GameNotStarted);
         }
      } else {
         return Err(MakePlayError::LobbyNotFound);
      }
   }

   fn do_reconnect(&mut self, message: &ReconnectMessage) -> Result<(), ReconnectError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if let Some(player) = lobby.players.get_mut(&message.player_id) {
            player.connection = Connection::Connected(self.out.clone());
            if let Some(ref gs) = lobby.game {
               /*let _ = serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::GameStartEvent(GameStartEvent {
                     hand: gs.get_hand(player.turn_number),
                     turn_number: player.turn_number,
                  }),
               );*/
               let _ = serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::PublicGameStateEvent(&gs.public_state()),
               );
            }

            Ok(())
         } else {
            Err(ReconnectError::PlayerNotFound)
         }
      } else {
         Err(ReconnectError::LobbyNotFound)
      }
   }
}

fn update_connected_player_info(
   connected_lobby_player: &mut Option<(LobbyId, PlayerId)>,
   lobbies: &mut HashMap<LobbyId, Lobby>,
   new_lobby_id: LobbyId,
   new_player_id: PlayerId,
) {
   disconnect_old_player(connected_lobby_player, lobbies);

   *connected_lobby_player = Some((new_lobby_id, new_player_id));
}

fn disconnect_old_player(connected_lobby_player: &Option<(LobbyId, PlayerId)>, lobbies: &mut HashMap<LobbyId, Lobby>) {
   if let Some((old_lobby_id, old_player_id)) = connected_lobby_player {
      if let Some(old_lobby) = lobbies.get_mut(&old_lobby_id) {
         if let Some(old_player) = old_lobby.players.get_mut(&old_player_id) {
            old_player.connection = Connection::Disconnected(Instant::now());
         }
      }
   }
}

fn report_make_play(gs: &GameState, players: &mut HashMap<PlayerId, Player>, id_of_last_player: PlayerId) {
   let public_gs = gs.public_state();
   for (id, player) in players {
      match player.connection {
         Connection::Connected(ref mut sender) => {
            if *id == id_of_last_player {
               let _ = serialize_and_send(
                  sender,
                  &PalaceOutMessage::HandEvent(HandEvent {
                     hand: gs.get_hand(player.turn_number),
                  }),
               );
            }
            let _ = serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
         }
         Connection::Disconnected(_) => (),
         Connection::Ai(ref mut ai) => {
            if *id == id_of_last_player {
               ai.on_hand_update(gs.get_hand(player.turn_number));
            }
            ai.on_game_state_update(&public_gs);
         }
      }
   }
}

fn report_choose_faceup(gs: &GameState, players: &mut HashMap<PlayerId, Player>, id_of_last_player: PlayerId) {
   let public_gs = gs.public_state();
   for (id, player) in players {
      match player.connection {
         Connection::Connected(ref mut sender) => {
            if *id == id_of_last_player {
               let _ = serialize_and_send(
                  sender,
                  &PalaceOutMessage::HandEvent(HandEvent {
                     hand: gs.get_hand(player.turn_number),
                  }),
               );
            }
            let _ = serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
         }
         Connection::Disconnected(_) => (),
         Connection::Ai(ref mut ai) => {
            if *id == id_of_last_player {
               ai.on_hand_update(gs.get_hand(player.turn_number));
            }
            ai.on_game_state_update(&public_gs);
         }
      }
   }
}

fn add_player(new_player: Player, player_id: PlayerId, lobby: &mut Lobby) {
   let new_player_name = new_player.name.clone();

   lobby.players.insert(player_id, new_player);

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
                  new_player_name: &new_player_name,
               }),
            );
         }
         Connection::Disconnected(_) => (),
         Connection::Ai(_) => (),
      }
   }
}

fn serialize_and_send(s: &mut Sender, message: &PalaceOutMessage) -> ws::Result<()> {
   match serde_json::to_vec(message) {
      Ok(bytes) => {
         debug!("Sending bytes (as string) {:?}", String::from_utf8_lossy(&bytes));
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
         std::thread::sleep(Duration::from_secs(30));
         let lobby_clean_start = Instant::now();
         let mut lobbies = thread_lobbies.write().unwrap();
         lobbies.retain(|_, lobby| {
            for player in lobby.players.values() {
               match player.connection {
                  Connection::Connected(_) => {
                     return true;
                  }
                  Connection::Disconnected(disconnection_time) => {
                     if disconnection_time.elapsed() < Duration::from_secs(30) {
                        return true;
                     }
                  }
                  Connection::Ai(_) => (),
               }
            }
            false
         });
         trace!("Lobby cleanup runtime: {:?}", lobby_clean_start.elapsed());
      });
   }

   // Spawn a thread to play AI games
   {
      let cloned_lobbies = lobbies.clone();
      std::thread::spawn(move || {
         ai_play_loop(&cloned_lobbies);
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
   use std::sync::mpsc;

   #[cfg(test)]
   use parking_lot::Mutex;

   #[cfg(test)]
   #[derive(Deserialize)]
   struct TestLobbyDisplay {
      name: String,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   struct TestNewLobbyResponse {
      player_id: String,
      lobby_id: String,
   }

   #[cfg(test)]
   #[derive(Debug, Deserialize)]
   enum TestNewLobbyError {
      LessThanTwoMaxPlayers,
      EmptyLobbyName,
      EmptyPlayerName,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   struct PlayerJoinEvent {
      pub total_num_players: u8,
      pub new_player_name: String,
   }

   #[derive(Deserialize)]
   #[cfg(test)]
   enum RequestAiError {
      NotLobbyOwner,
      LessThanOneAiRequested,
      LobbyNotFound,
      LobbyTooSmall,
   }

   #[cfg(test)]
   #[derive(Deserialize)]
   enum TestInMessage {
      ListLobbiesResponse(Box<[TestLobbyDisplay]>),
      NewLobbyResponse(Result<TestNewLobbyResponse, TestNewLobbyError>),
      PlayerJoinEvent(PlayerJoinEvent),
      RequestAiResponse(Result<(), RequestAiError>),
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
   struct TestRequestAiMessage {
      pub lobby_id: String,
      pub player_id: String,
      pub num_ai: u8,
   }

   #[cfg(test)]
   #[derive(Serialize)]
   enum TestOutMessage {
      NewLobby(TestNewLobbyMessage),
      RequestAi(TestRequestAiMessage),
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
            if let Ok(bytes) = self.to_send_messages.lock().try_recv() {
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

   #[cfg(test)]
   static SERVER_UP: Mutex<bool> = Mutex::new(false);

   #[cfg(test)]
   fn ensure_test_server_up() {
      let mut server_up = SERVER_UP.lock();
      if !*server_up {
         std::thread::spawn(move || {
            run_server("127.0.0.1:3013");
         });
         // TODO ideally this would be a retry ready check
         std::thread::sleep(Duration::from_secs(5));
         *server_up = true
      }
   }

   #[test]
   fn test_lobbies_clean_up() {
      ensure_test_server_up();

      // Create a lobby
      {
         let mut tc = TestClient::new();
         tc.send(TestOutMessage::NewLobby(TestNewLobbyMessage {
            player_name: "TestClient".into(),
            lobby_name: "JunkLobby".into(),
            password: "".into(),
            max_players: 4,
         }));
         let nlr = tc.get();
         match nlr {
            TestInMessage::NewLobbyResponse(r) => assert!(r.is_ok()),
            _ => panic!("Expected new lobby response"),
         }
         tc.disconnect();
      }

      std::thread::sleep(Duration::from_secs(30));

      // Ensure lobby is cleaned up
      {
         let mut tc = TestClient::new();
         tc.send(TestOutMessage::ListLobbies);
         let llr = tc.get();
         match llr {
            TestInMessage::ListLobbiesResponse(r) => assert!(r.iter().find(|x| x.name == "JunkLobby").is_none()),
            _ => panic!("Expected list lobbies response"),
         }
      }
   }

   #[test]
   fn test_bots_join_lobby_after_request() {
      ensure_test_server_up();

      let mut tc = TestClient::new();
      // Create a lobby
      let (player_id, lobby_id) = {
         tc.send(TestOutMessage::NewLobby(TestNewLobbyMessage {
            player_name: "TestClient".into(),
            lobby_name: "TestLobby".into(),
            password: "foo".into(),
            max_players: 4,
         }));
         let nlr = tc.get();
         match nlr {
            TestInMessage::NewLobbyResponse(r) => {
               let inner = r.expect("New lobby failed");
               (inner.player_id, inner.lobby_id)
            }
            _ => panic!("Expected new lobby response"),
         }
      };

      // Request 3 AI
      {
         tc.send(TestOutMessage::RequestAi(TestRequestAiMessage {
            num_ai: 3,
            player_id: player_id,
            lobby_id: lobby_id,
         }));

         // Ensure that three AI join
         for _ in 0..3 {
            match tc.get() {
               TestInMessage::PlayerJoinEvent(_) => (),
               _ => panic!("Expected PlayerJoinEvent"),
            }
         }

         // Ensure the AI response is OK
         match tc.get() {
            TestInMessage::RequestAiResponse(r) => assert!(r.is_ok()),
            _ => panic!("Expected RequestAiResponse"),
         }
      }
   }
}
