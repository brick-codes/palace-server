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

const TURN_TIMER_SECS: u64 = 50;
const EMPTY_LOBBY_PRUNE_THRESHOLD_SECS: u64 = 30;

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
   players_by_turn_num: HashMap<u8, PlayerId>,
   spectators: Vec<Sender>,
   max_players: u8,
   password: String,
   game: Option<GameState>,
   owner: PlayerId,
   name: String,
   creation_time: Instant,
}

fn next_public_id(players_by_public_id: &HashMap<u8, PlayerId>) -> u8 {
   let mut id: u8 = 0;
   while players_by_public_id.contains_key(&id) {
      id += 1;
   }
   id
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
   kicked: bool,
}

impl Player {
   fn is_ai(&self) -> bool {
      match self.connection {
         Connection::Ai(_) => true,
         _ => false,
      }
   }
}

enum ConnectedUser {
   Player((LobbyId, PlayerId)),
   Spectator(LobbyId),
}

struct Server {
   out: Sender,
   lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>>,
   connected_user: Option<ConnectedUser>,
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

fn ai_play(lobbies: &mut HashMap<LobbyId, Lobby>) {
   for lobby in &mut lobbies.values_mut() {
      if let Some(ref mut gs) = lobby.game {
         if let Some(player_id) = lobby.players_by_turn_num.get(&gs.active_player) {
            match gs.cur_phase {
               game::Phase::Setup => {
                  let faceup_three = match lobby.players.get_mut(player_id).unwrap().connection {
                     Connection::Ai(ref mut ai) => ai.choose_three_faceup(),
                     _ => continue,
                  };
                  match gs.choose_three_faceup(faceup_three.0, faceup_three.1, faceup_three.2) {
                     Ok(()) => {
                        report_choose_faceup(&gs, &mut lobby.players, *player_id);
                     }
                     Err(_) => {
                        let mut players = HashMap::new();
                        for player in lobby.players.values() {
                           players.insert(player.turn_number, player.name.clone());
                        }
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
                                    players: &players,
                                 });
                                 ai.on_game_state_update(&gs.public_state());
                              }
                           }
                           _ => unreachable!(),
                        }
                     }
                  }
               }
               game::Phase::Play => {
                  let play = match lobby.players.get_mut(player_id).unwrap().connection {
                     Connection::Ai(ref mut ai) => if gs.hands[gs.active_player as usize].is_empty()
                        && gs.face_up_three[gs.active_player as usize].is_empty()
                     {
                        vec![].into_boxed_slice()
                     } else {
                        ai.take_turn()
                     },
                     _ => continue,
                  };
                  match gs.make_play(play) {
                     Ok(()) => {
                        report_make_play(&gs, &mut lobby.players, *player_id);
                     }
                     Err(_) => {
                        let mut players = HashMap::new();
                        for player in lobby.players.values() {
                           players.insert(player.turn_number, player.name.clone());
                        }
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
                                    players: &players,
                                 });
                                 ai.on_game_state_update(&gs.public_state());
                              }
                           }
                           _ => unreachable!(),
                        }
                     }
                  }
               }
               game::Phase::Complete => {
                  continue;
               }
            }
         }
      } else {
         continue;
      }
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
      if let Some(ref connected_user_details) = self.connected_user {
         disconnect_old_player(connected_user_details, &mut lobbies, self.out.connection_id());
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
         PalaceInMessage::KickPlayer(message) => {
            let response = PalaceOutMessage::KickPlayerResponse(self.do_kick_player(&message));
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
         } else if lobby.game.is_some() {
            Err(RequestAiError::GameInProgress)
         } else {
            for _ in 0..message.num_ai {
               let player_id = PlayerId(rand::random());
               let ai: Box<PalaceAi + Send + Sync> = Box::new(ai::random::new());
               add_player(
                  Player {
                     name: ai::get_bot_name(),
                     connection: Connection::Ai(ai),
                     turn_number: next_public_id(&lobby.players_by_turn_num),
                     kicked: false,
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
      let mut players_by_public_id = HashMap::new();
      players.insert(
         player_id,
         Player {
            name: message.player_name,
            connection: Connection::Connected(self.out.clone()),
            turn_number: 0,
            kicked: false,
         },
      );
      players_by_public_id.insert(0, player_id);
      lobbies.insert(
         lobby_id,
         Lobby {
            players,
            players_by_turn_num: players_by_public_id,
            game: None,
            password: message.password,
            name: message.lobby_name,
            owner: player_id,
            max_players: message.max_players,
            creation_time: Instant::now(),
            spectators: Vec::new(),
         },
      );

      update_connected_player_info(&mut self.connected_user, &mut lobbies, ConnectedUser::Player((lobby_id, player_id)), self.out.connection_id());

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
               turn_number: next_public_id(&lobby.players_by_turn_num),
               kicked: false,
            },
            player_id,
            lobby,
         );

         player_id
      } else {
         return Err(JoinLobbyError::LobbyNotFound);
      };

      update_connected_player_info(
         &mut self.connected_user,
         &mut lobbies,
         ConnectedUser::Player((message.lobby_id, new_player_id)),
         self.out.connection_id(),
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
         for (id, player) in &mut lobby.players {
            player.turn_number = turn_numbers.next().unwrap();
            lobby.players_by_turn_num.insert(player.turn_number, *id);
            // @Performance: we can avoid cloning here
            // because we don't modify the hashmap before we send the data.
            // the problem is convincing that to the rust compiler
            // which sees us mutably borrowing the hashmap
            // (to send data out). So, use unsafe?
            players.insert(player.turn_number, player.name.clone());
         }

         // Send out game start events
         for player in lobby.players.values_mut() {
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
         // @Performance we construct this but throw it away if the user can't reconnect
         // (was kicked)
         let mut players = HashMap::new();
         for player in lobby.players.values() {
            players.insert(player.turn_number, player.name.clone());
         }
         if let Some(player) = lobby.players.get_mut(&message.player_id) {
            if player.kicked {
               return Err(ReconnectError::PlayerKicked);
            }

            player.connection = Connection::Connected(self.out.clone());
            if let Some(ref gs) = lobby.game {
               let _ = serialize_and_send(
                  &mut self.out,
                  &PalaceOutMessage::GameStartEvent(GameStartEvent {
                     hand: gs.get_hand(player.turn_number),
                     turn_number: player.turn_number,
                     players: &players,
                  }),
               );
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

   fn do_kick_player(&mut self, message: &KickPlayerMessage) -> Result<(), KickPlayerError> {
      if message.slot == 0 {
         return Err(KickPlayerError::CantKickLobbyOwner);
      }

      let mut lobbies = self.lobbies.write().unwrap();

      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         if lobby.owner != message.player_id {
            Err(KickPlayerError::NotLobbyOwner)
         } else if let Some(player_id) = lobby.players_by_turn_num.get(&message.slot) {
            match lobby.game {
               Some(_) => {
                  let player = lobby.players.get_mut(&player_id).unwrap();
                  match player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::Kicked));
                        player.connection = Connection::Disconnected(Instant::now());
                        player.kicked = true;
                        Ok(())
                     }
                     Connection::Disconnected(_) => {
                        player.kicked = true;
                        Ok(())
                     }
                     Connection::Ai(_) => Err(KickPlayerError::CantKickAiDuringGame),
                  }
               }
               None => {
                  remove_player(*player_id, lobby, Some(LobbyCloseEvent::Kicked));
                  Ok(())
               }
            }
         } else {
            Err(KickPlayerError::TargetPlayerNotFound)
         }
      } else {
         Err(KickPlayerError::LobbyNotFound)
      }
   }
}

fn update_connected_player_info(
   connected_user: &mut Option<ConnectedUser>,
   lobbies: &mut HashMap<LobbyId, Lobby>,
   new_connection: ConnectedUser,
   our_sender_id: u32,
) {
   if let Some(ref connected_user_details) = connected_user {
      disconnect_old_player(connected_user_details, lobbies, our_sender_id);
   }

   *connected_user = Some(new_connection);
}

fn disconnect_old_player(connected_user: &ConnectedUser, lobbies: &mut HashMap<LobbyId, Lobby>, our_sender_id: u32) {
   match connected_user {
      ConnectedUser::Player((old_lobby_id, old_player_id)) => {
         if let Some(old_lobby) = lobbies.get_mut(&old_lobby_id) {
            if old_lobby.game.is_none() {
               if old_lobby.owner == *old_player_id {
                  for (_, old_player) in old_lobby.players.iter_mut().filter(|(id, _)| *id != old_player_id) {
                     match old_player.connection {
                        Connection::Connected(ref mut sender) => {
                           let _ =
                              serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::OwnerLeft));
                        }
                        Connection::Disconnected(_) => (),
                        Connection::Ai(_) => (),
                     }
                  }
                  lobbies.remove(&old_lobby_id);
               } else {
                  remove_player(*old_player_id, old_lobby, None);
               }
            } else if let Some(old_player) = old_lobby.players.get_mut(&old_player_id) {
               old_player.connection = Connection::Disconnected(Instant::now());
            }
         }
      }
      ConnectedUser::Spectator(old_lobby_id) => {
         if let Some(old_lobby) = lobbies.get_mut(&old_lobby_id) {
            old_lobby.spectators.retain(|x| x.connection_id() != our_sender_id);
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
               let _ = serialize_and_send(sender, &PalaceOutMessage::HandEvent(gs.get_hand(player.turn_number)));
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
               let _ = serialize_and_send(sender, &PalaceOutMessage::HandEvent(gs.get_hand(player.turn_number)));
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

   let turn_number = new_player.turn_number;
   lobby.players_by_turn_num.insert(turn_number, player_id);
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
                  slot: turn_number,
               }),
            );
         }
         Connection::Disconnected(_) => (),
         Connection::Ai(_) => (),
      }
   }
}

fn remove_player(old_player_id: PlayerId, lobby: &mut Lobby, opt_event: Option<LobbyCloseEvent>) {
   let old_player_opt = lobby.players.remove(&old_player_id);

   if let Some(mut old_player) = old_player_opt {
      lobby.players_by_turn_num.remove(&old_player.turn_number);

      if let Some(event) = opt_event {
         match old_player.connection {
            Connection::Connected(ref mut sender) => {
               let _ = serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(event));
            }
            Connection::Disconnected(_) => (),
            Connection::Ai(_) => (),
         }
      }

      let new_num_players = lobby.players.len() as u8;
      for player in lobby.players.values_mut() {
         match player.connection {
            Connection::Connected(ref mut sender) => {
               let _ = serialize_and_send(
                  sender,
                  &PalaceOutMessage::PlayerLeaveEvent(PlayerLeaveEvent {
                     total_num_players: new_num_players,
                     slot: old_player.turn_number,
                  }),
               );
            }
            Connection::Disconnected(_) => (),
            Connection::Ai(_) => (),
         }
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

pub fn run_server(address: &'static str) {
   pretty_env_logger::init();
   // @Performance this could be a concurrent hashmap
   let lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>> = Arc::new(RwLock::new(HashMap::new()));

   // Kick idle players
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(Duration::from_secs(1));
         //let kick_idle_start = Instant::now();
         {
            let mut lobbies = thread_lobbies.write().unwrap();
            for lobby in lobbies.values_mut() {
               if let Some(ref mut gs) = lobby.game {
                  if gs.cur_phase == game::Phase::Complete {
                     continue;
                  }

                  if gs.last_turn_start.elapsed() >= Duration::from_secs(TURN_TIMER_SECS) {
                     // @Performance we do this and then throw it away if it's not a bot that isn't Random...
                     let mut players = HashMap::new();
                     for player in lobby.players.values() {
                        players.insert(player.turn_number, player.name.clone());
                     }

                     let player_id = lobby.players_by_turn_num[&gs.active_player];
                     let player = lobby.players.get_mut(&player_id).unwrap();
                     match player.connection {
                        Connection::Connected(ref mut sender) => {
                           let _ = serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::Afk));
                           player.connection = Connection::Disconnected(Instant::now());
                        }
                        Connection::Disconnected(_) => (),
                        Connection::Ai(ref mut ai) => {
                           error!(
                              "Bot (strategy: {}) failed to take its turn within time limit",
                              ai.strategy_name()
                           );
                           if ai.strategy_name() != "Random" {
                              info!("Falling back to Random");
                              *ai = Box::new(ai::random::new());
                              ai.on_game_start(GameStartEvent {
                                 hand: gs.get_hand(player.turn_number),
                                 turn_number: player.turn_number,
                                 players: &players,
                              });
                              ai.on_game_state_update(&gs.public_state());
                           }
                        }
                     }
                  }
               }
            }
         }
         //trace!("Kick idle runtime: {:?}", kick_idle_start.elapsed());
      });
   }

   // Prune empty lobbies
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(Duration::from_secs(30));
         let lobby_clean_start = Instant::now();
         {
            let mut lobbies = thread_lobbies.write().unwrap();
            lobbies.retain(|_, lobby| {
               for player in lobby.players.values() {
                  match player.connection {
                     Connection::Connected(_) => {
                        return true;
                     }
                     Connection::Disconnected(disconnection_time) => {
                        if disconnection_time.elapsed() < Duration::from_secs(EMPTY_LOBBY_PRUNE_THRESHOLD_SECS) {
                           return true;
                        }
                     }
                     Connection::Ai(_) => (),
                  }
               }
               false
            });
         }
         trace!("Lobby cleanup runtime: {:?}", lobby_clean_start.elapsed());
      });
   }

   // Update AI
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(Duration::from_millis(100));
         //let ai_loop_start = Instant::now();
         ai_play(&mut thread_lobbies.write().unwrap());
         //trace!("AI runtime: {:?}", ai_loop_start.elapsed());
      });
   }

   ws::listen(address, |out| Server {
      out,
      lobbies: lobbies.clone(),
      connected_user: None,
   }).unwrap()
}
