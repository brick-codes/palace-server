mod ai;
mod data;
mod game;

use crate::ai::PalaceAi;
use crate::data::*;
use crate::game::GameState;
use log::{debug, error, info, trace};
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Deserializer, Serializer};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use ws::{CloseCode, Handler, Handshake, Message, Sender};

const EMPTY_LOBBY_PRUNE_THRESHOLD_SECS: u64 = 30;
const PLAYER_NAME_LIMIT: usize = 20;
const LOBBY_NAME_LIMIT: usize = 20;
const PASSWORD_LIMIT: usize = 20;

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
   turn_timer: Duration,
   games_completed: u64,
}

impl Lobby {
   pub(crate) fn display(&self, lobby_id: &LobbyId) -> LobbyDisplay {
      LobbyDisplay {
         cur_players: self.players.len() as u8,
         ai_players: self.players.values().filter(|p| p.is_requested_ai()).count() as u8,
         max_players: self.max_players,
         started: self.game.is_some(),
         has_password: !self.password.is_empty(),
         owner: &self.players[&self.owner].name,
         name: &self.name,
         age: self.creation_time.elapsed().as_secs(),
         lobby_id: *lobby_id,
         cur_spectators: self.spectators.len() as u8,
         turn_timer: self.turn_timer.as_secs() as u8,
         games_completed: self.games_completed,
      }
   }
}

#[derive(Serialize)]
pub(crate) struct LobbyDisplay<'a> {
   pub cur_players: u8,
   pub ai_players: u8,
   pub max_players: u8,
   pub started: bool,
   pub has_password: bool,
   pub owner: &'a str,
   pub name: &'a str,
   pub age: u64,
   pub lobby_id: LobbyId,
   pub cur_spectators: u8,
   pub turn_timer: u8,
   pub games_completed: u64,
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
   Disconnected(DisconnectedState),
   Ai(AiState),
}

struct AiState {
   core: Box<dyn PalaceAi + Send + Sync>,
   is_clandestine: bool,
}

struct DisconnectedState {
   time: Instant,
   reason: DisconnectedReason,
}

#[derive(PartialEq)]
enum DisconnectedReason {
   Kicked,
   TimedOut,
   Left,
}

struct Player {
   name: String,
   connection: Connection,
   turn_number: u8,
}

impl Player {
   fn is_requested_ai(&self) -> bool {
      match &self.connection {
         Connection::Ai(ai) => !ai.is_clandestine,
         _ => false,
      }
   }

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
                     Connection::Ai(ref mut ai) => ai.core.choose_three_faceup(),
                     _ => continue,
                  };
                  match gs.choose_three_faceup(faceup_three.0, faceup_three.1, faceup_three.2) {
                     Ok(()) => {
                        report_choose_faceup(&gs, &mut lobby.players, *player_id);
                     }
                     Err(_) => {
                        let player = lobby.players.get_mut(player_id).unwrap();
                        match player.connection {
                           Connection::Ai(ref mut ai) => {
                              error!(
                                 "Bot (strategy: {}) failed to choose three faceup",
                                 ai.core.strategy_name()
                              );
                              if ai.core.strategy_name() != "Random" {
                                 info!("Falling back to Random");
                                 ai.core = Box::new(ai::random::new());
                                 ai.core.on_game_start(GameStartEvent {
                                    hand: gs.get_hand(player.turn_number),
                                    turn_number: player.turn_number,
                                    players: &HashMap::new(), // Random doesn't need players
                                 });
                                 ai.core.on_game_state_update(&gs.public_state());
                              }
                           }
                           _ => unreachable!(),
                        }
                     }
                  }
               }
               game::Phase::Play => {
                  let play = match lobby.players.get_mut(player_id).unwrap().connection {
                     Connection::Ai(ref mut ai) => ai::get_play(&gs, &mut *ai.core),
                     _ => continue,
                  };
                  match gs.make_play(play) {
                     Ok(game_finished) => {
                        report_make_play(&gs, &mut lobby.players, &mut lobby.spectators, *player_id);
                        if game_finished {
                           end_game(lobby);
                        }
                     }
                     Err(_) => {
                        let player = lobby.players.get_mut(player_id).unwrap();
                        match player.connection {
                           Connection::Ai(ref mut ai) => {
                              error!("Bot (strategy: {}) failed to make play", ai.core.strategy_name());
                              if ai.core.strategy_name() != "Random" {
                                 info!("Falling back to Random");
                                 ai.core = Box::new(ai::random::new());
                                 ai.core.on_game_start(GameStartEvent {
                                    hand: gs.get_hand(player.turn_number),
                                    turn_number: player.turn_number,
                                    players: &HashMap::new(), // Random doesn't need players
                                 });
                                 ai.core.on_game_state_update(&gs.public_state());
                              }
                           }
                           _ => unreachable!(),
                        }
                     }
                  }
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
         PalaceInMessage::ListLobbies(message) => {
            let lobbies = self.lobbies.read().unwrap();
            // @Performance we should be able to serialize with Serializer::collect_seq
            // and avoid collecting into a vector
            serialize_and_send(
               &mut self.out,
               &PalaceOutMessage::ListLobbiesResponse(ListLobbyResponse {
                  lobbies: &lobbies
                     .iter()
                     .skip(message.page as usize * 50)
                     .map(|(k, v)| v.display(k))
                     .collect::<Vec<_>>(),
                  has_next_page: lobbies.len() as u64 > (message.page + 1) * 50,
               }),
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
         PalaceInMessage::SpectateLobby(message) => {
            // See note on JoinLobby above
            match self.do_spectate_lobby(message) {
               Ok(()) => Ok(()),
               Err(e) => serialize_and_send(&mut self.out, &PalaceOutMessage::SpectateLobbyResponse(Err(e))),
            }
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
                     connection: Connection::Ai(AiState {
                        core: ai,
                        is_clandestine: false,
                     }),
                     turn_number: next_public_id(&lobby.players_by_turn_num),
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

      if message.lobby_name.len() > LOBBY_NAME_LIMIT {
         return Err(NewLobbyError::LobbyNameTooLong);
      }

      if message.player_name.len() > PLAYER_NAME_LIMIT {
         return Err(NewLobbyError::PlayerNameTooLong);
      }

      if message.password.len() > PASSWORD_LIMIT {
         return Err(NewLobbyError::PasswordTooLong);
      }

      let mut lobbies = self.lobbies.write().unwrap();
      let (lobby_id, player_id) = create_lobby(
         &mut lobbies,
         Connection::Connected(self.out.clone()),
         message.lobby_name,
         message.player_name,
         message.password,
         message.max_players,
         message.turn_timer,
      );

      update_connected_player_info(
         &mut self.connected_user,
         &mut lobbies,
         ConnectedUser::Player((lobby_id, player_id)),
         self.out.connection_id(),
      );

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

      if message.player_name.len() > PLAYER_NAME_LIMIT {
         return Err(JoinLobbyError::PlayerNameTooLong);
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
               num_spectators: lobby.spectators.len() as u8,
               turn_timer: lobby.turn_timer.as_secs() as u8,
            })),
         );

         add_player(
            Player {
               name: message.player_name,
               connection: Connection::Connected(self.out.clone()),
               turn_number: next_public_id(&lobby.players_by_turn_num),
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

   fn do_spectate_lobby(&mut self, message: LobbyId) -> Result<(), SpectateLobbyError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message) {
         if lobby.spectators.len() as u8 == std::u8::MAX {
            return Err(SpectateLobbyError::SpectateLobbyFull);
         }

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
            &PalaceOutMessage::SpectateLobbyResponse(Ok(SpectateLobbyResponse {
               lobby_players,
               max_players: lobby.max_players,
               num_spectators: lobby.spectators.len() as u8 + 1,
               turn_timer: lobby.turn_timer.as_secs() as u8,
            })),
         );

         if let Some(ref gs) = lobby.game {
            let mut players = HashMap::new();
            for player in lobby.players.values() {
               players.insert(player.turn_number, player.name.clone());
            }
            let _ = serialize_and_send(
               &mut self.out,
               &PalaceOutMessage::SpectateGameStartEvent(SpectateGameStartEvent { players: &players }),
            );
            let _ = serialize_and_send(
               &mut self.out,
               &PalaceOutMessage::PublicGameStateEvent(&gs.public_state()),
            );
         }

         for player in lobby.players.values_mut() {
            match player.connection {
               Connection::Connected(ref mut sender) => {
                  let _ = serialize_and_send(sender, &PalaceOutMessage::SpectatorJoinEvent(()));
               }
               Connection::Disconnected(_) => (),
               Connection::Ai(_) => (),
            }
         }
         for sender in &mut lobby.spectators {
            let _ = serialize_and_send(sender, &PalaceOutMessage::SpectatorJoinEvent(()));
         }

         lobby.spectators.push(self.out.clone());

         update_connected_player_info(
            &mut self.connected_user,
            &mut lobbies,
            ConnectedUser::Spectator(message),
            self.out.connection_id(),
         );

         Ok(())
      } else {
         Err(SpectateLobbyError::LobbyNotFound)
      }
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

         start_game(lobby);

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
               Ok(game_finished) => {
                  report_make_play(&gs, &mut lobby.players, &mut lobby.spectators, message.player_id);
                  if game_finished {
                     end_game(lobby);
                  }
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

   fn do_reconnect(&mut self, message: &ReconnectMessage) -> Result<ReconnectResponse, ReconnectError> {
      let mut lobbies = self.lobbies.write().unwrap();
      if let Some(lobby) = lobbies.get_mut(&message.lobby_id) {
         // @Performance we construct this but throw it away if the user can't reconnect
         // (was kicked)
         let mut players = HashMap::new();
         for player in lobby.players.values() {
            players.insert(player.turn_number, player.name.clone());
         }
         if let Some(player) = lobby.players.get_mut(&message.player_id) {
            if let Connection::Disconnected(ds) = &player.connection {
               if ds.reason == DisconnectedReason::Kicked {
                  return Err(ReconnectError::PlayerKicked);
               }
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

            Ok(ReconnectResponse {
               max_players: lobby.max_players,
               num_spectators: lobby.spectators.len() as u8,
               turn_timer: lobby.turn_timer.as_secs() as u8,
            })
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
                  match &mut player.connection {
                     Connection::Connected(ref mut sender) => {
                        let _ = serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::Kicked));
                        player.connection = Connection::Disconnected(DisconnectedState {
                           time: Instant::now(),
                           reason: DisconnectedReason::Kicked,
                        });
                        Ok(())
                     }
                     Connection::Disconnected(ref mut ds) => {
                        ds.reason = DisconnectedReason::Kicked;
                        Ok(())
                     }
                     Connection::Ai(ref ai) => {
                        if ai.is_clandestine {
                           player.connection = Connection::Disconnected(DisconnectedState {
                              time: Instant::now(),
                              reason: DisconnectedReason::Kicked,
                           });
                           Ok(())
                        } else {
                           Err(KickPlayerError::CantKickAiDuringGame)
                        }
                     }
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

fn create_lobby(
   lobbies: &mut HashMap<LobbyId, Lobby>,
   connection: Connection,
   lobby_name: String,
   player_name: String,
   password: String,
   max_players: u8,
   turn_timer: u8,
) -> (LobbyId, PlayerId) {
   let lobby_id = LobbyId(rand::random());
   let player_id = PlayerId(rand::random());
   let mut players = HashMap::new();
   let mut players_by_public_id = HashMap::new();
   players.insert(
      player_id,
      Player {
         name: player_name,
         connection,
         turn_number: 0,
      },
   );
   players_by_public_id.insert(0, player_id);
   lobbies.insert(
      lobby_id,
      Lobby {
         players,
         players_by_turn_num: players_by_public_id,
         game: None,
         password,
         name: lobby_name,
         owner: player_id,
         max_players,
         creation_time: Instant::now(),
         spectators: Vec::new(),
         turn_timer: Duration::from_secs(u64::from(turn_timer)),
         games_completed: 0,
      },
   );

   (lobby_id, player_id)
}

fn start_game(lobby: &mut Lobby) {
   let num_players = lobby.players.len() as u8;
   let gs = GameState::new(num_players);
   lobby.game = Some(gs);

   let public_gs = lobby.game.as_ref().unwrap().public_state();

   let mut turn_numbers: Vec<u8> = (0..num_players).collect();
   turn_numbers.shuffle(&mut thread_rng());
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
            ai.core.on_game_start(GameStartEvent {
               hand: lobby.game.as_ref().unwrap().get_hand(player.turn_number),
               turn_number: player.turn_number,
               players: &players,
            });
            ai.core.on_game_state_update(&public_gs);
         }
      }
   }
   for sender in &mut lobby.spectators {
      let _ = serialize_and_send(
         sender,
         &PalaceOutMessage::SpectateGameStartEvent(SpectateGameStartEvent { players: &players }),
      );
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
                           let _ = serialize_and_send(
                              sender,
                              &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::OwnerLeft),
                           );
                        }
                        Connection::Disconnected(_) => (),
                        Connection::Ai(_) => (),
                     }
                  }
                  for sender in &mut old_lobby.spectators {
                     let _ = serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::OwnerLeft));
                  }
                  lobbies.remove(&old_lobby_id);
               } else {
                  remove_player(*old_player_id, old_lobby, None);
               }
            } else if let Some(old_player) = old_lobby.players.get_mut(&old_player_id) {
               old_player.connection = Connection::Disconnected(DisconnectedState {
                  time: Instant::now(),
                  reason: DisconnectedReason::Left,
               });
            }
         }
      }
      ConnectedUser::Spectator(old_lobby_id) => {
         if let Some(old_lobby) = lobbies.get_mut(&old_lobby_id) {
            old_lobby.spectators.retain(|x| x.connection_id() != our_sender_id);

            for player in old_lobby.players.values_mut() {
               match player.connection {
                  Connection::Connected(ref mut sender) => {
                     let _ = serialize_and_send(sender, &PalaceOutMessage::SpectatorLeaveEvent(()));
                  }
                  Connection::Disconnected(_) => (),
                  Connection::Ai(_) => (),
               }
            }
            for sender in &mut old_lobby.spectators {
               let _ = serialize_and_send(sender, &PalaceOutMessage::SpectatorLeaveEvent(()));
            }
         }
      }
   }
}

fn report_make_play(
   gs: &GameState,
   players: &mut HashMap<PlayerId, Player>,
   spectators: &mut [Sender],
   id_of_last_player: PlayerId,
) {
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
               ai.core.on_hand_update(gs.get_hand(player.turn_number));
            }
            ai.core.on_game_state_update(&public_gs);
         }
      }
   }
   for sender in spectators {
      let _ = serialize_and_send(sender, &PalaceOutMessage::PublicGameStateEvent(&public_gs));
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
               ai.core.on_hand_update(gs.get_hand(player.turn_number));
            }
            ai.core.on_game_state_update(&public_gs);
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
   for (_, player) in lobby.players.iter_mut().filter(|(id, _)| **id != player_id) {
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
   for sender in &mut lobby.spectators {
      let _ = serialize_and_send(
         sender,
         &PalaceOutMessage::PlayerJoinEvent(PlayerJoinEvent {
            total_num_players: new_num_players,
            new_player_name: &new_player_name,
            slot: turn_number,
         }),
      );
   }
}

/// This REMOVES players (from the turn order.) Not meant for game in progress
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
      for sender in &mut lobby.spectators {
         let _ = serialize_and_send(
            sender,
            &PalaceOutMessage::PlayerLeaveEvent(PlayerLeaveEvent {
               total_num_players: new_num_players,
               slot: old_player.turn_number,
            }),
         );
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

/// Panics if game is not in progress
fn end_game(lobby: &mut Lobby) {
   let gs = lobby.game.as_ref().unwrap();
   let mut players_to_remove = Vec::new();
   for (id, player) in &mut lobby.players {
      match player.connection {
         Connection::Disconnected(_) => {
            players_to_remove.push(*id);
         }
         Connection::Connected(ref mut sender) => {
            let _ = serialize_and_send(sender, &PalaceOutMessage::GameCompleteEvent(&gs.out_players));
         }
         Connection::Ai(_) => (),
      }
   }
   for sender in &mut lobby.spectators {
      let _ = serialize_and_send(sender, &PalaceOutMessage::GameCompleteEvent(&gs.out_players));
   }
   lobby.game = None;
   lobby.games_completed += 1;
   players_to_remove
      .into_iter()
      .for_each(|id| remove_player(id, lobby, None));
}

pub fn run_server(address: &'static str) {
   // @Performance this could be a concurrent hashmap
   let lobbies: Arc<RwLock<HashMap<LobbyId, Lobby>>> = Arc::new(RwLock::new(HashMap::new()));

   // Kick / take turns for idle players
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(Duration::from_millis(100));
         //let kick_idle_start = Instant::now();
         {
            let mut lobbies = thread_lobbies.write().unwrap();
            for lobby in lobbies.values_mut() {
               if let Some(ref mut gs) = lobby.game {
                  if lobby.turn_timer.as_secs() == 0 {
                     continue;
                  }

                  let player_id = lobby.players_by_turn_num[&gs.active_player];
                  let timed_out_or_kicked = match &lobby.players[&player_id].connection {
                     Connection::Disconnected(ds) => {
                        ds.reason == DisconnectedReason::Kicked || ds.reason == DisconnectedReason::TimedOut
                     }
                     _ => false,
                  };

                  if gs.last_turn_start.elapsed() >= lobby.turn_timer || timed_out_or_kicked {
                     // Update connection, if needed
                     {
                        let player = lobby.players.get_mut(&player_id).unwrap();
                        match player.connection {
                           Connection::Connected(ref mut sender) => {
                              let _ =
                                 serialize_and_send(sender, &PalaceOutMessage::LobbyCloseEvent(LobbyCloseEvent::Afk));
                              player.connection = Connection::Disconnected(DisconnectedState {
                                 time: Instant::now(),
                                 reason: DisconnectedReason::TimedOut,
                              });
                           }
                           Connection::Disconnected(ref mut dc) => {
                              // Elevate their disconnected status to timed out
                              // if they had left, so that their turns start
                              // being taken instantaneously
                              if dc.reason == DisconnectedReason::Left {
                                 dc.reason = DisconnectedReason::TimedOut;
                              }
                           }
                           Connection::Ai(ref mut ai) => {
                              error!(
                                 "Bot (strategy: {}) failed to take its turn within time limit",
                                 ai.core.strategy_name()
                              );
                              if ai.core.strategy_name() != "Random" {
                                 info!("Falling back to Random");
                                 ai.core = Box::new(ai::random::new());
                                 ai.core.on_game_start(GameStartEvent {
                                    hand: gs.get_hand(player.turn_number),
                                    turn_number: player.turn_number,
                                    players: &HashMap::new(), // Random doesn't need players
                                 });
                                 ai.core.on_game_state_update(&gs.public_state());
                              }
                           }
                        }
                     }

                     // Make a random play
                     // The reason why we have to do this here instead of letting the
                     // AI play loop pick it up is because we want to avoid the scenario
                     // in which a user gets kicked and immediately reconnects, before the AI
                     // play loop kicks in, therefore circumventing the turn timer
                     match gs.cur_phase {
                        game::Phase::Setup => {
                           let mut ai = Box::new(ai::random::new());
                           ai.on_game_start(GameStartEvent {
                              hand: gs.get_hand(gs.active_player),
                              turn_number: gs.active_player,
                              players: &HashMap::new(), // Random doesn't need players
                           });
                           ai.on_game_state_update(&gs.public_state());
                           let faceup_three = ai.choose_three_faceup();
                           gs.choose_three_faceup(faceup_three.0, faceup_three.1, faceup_three.2)
                              .unwrap();
                           report_choose_faceup(gs, &mut lobby.players, player_id);
                        }
                        game::Phase::Play => {
                           let mut ai = Box::new(ai::random::new());
                           ai.on_game_start(GameStartEvent {
                              hand: gs.get_hand(gs.active_player),
                              turn_number: gs.active_player,
                              players: &HashMap::new(), // Random doesn't need players
                           });
                           ai.on_game_state_update(&gs.public_state());
                           let play = ai::get_play(&gs, &mut *ai);
                           let must_end_game = gs.make_play(play).unwrap();
                           report_make_play(gs, &mut lobby.players, &mut lobby.spectators, player_id);
                           if must_end_game {
                              end_game(lobby);
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
               if !lobby.spectators.is_empty() {
                  return true;
               }
               for player in lobby.players.values() {
                  match &player.connection {
                     Connection::Connected(_) => {
                        return true;
                     }
                     Connection::Disconnected(ds) => {
                        if ds.time.elapsed() < Duration::from_secs(EMPTY_LOBBY_PRUNE_THRESHOLD_SECS) {
                           return true;
                        }
                     }
                     Connection::Ai(ref ai) => {
                        if ai.is_clandestine {
                           return true;
                        }
                     }
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
         //trace!("AI play runtime: {:?}", ai_loop_start.elapsed());
      });
   }

   // Clandestine AI
   // Ideally, each sub-function should be on a seperate timer
   // @TODO probably pending an async rewrite with tokio-tungstenite
   {
      let thread_lobbies = lobbies.clone();
      std::thread::spawn(move || loop {
         std::thread::sleep(Duration::from_millis(rand::thread_rng().gen_range(100, 10000)));

         let mut lobbies = thread_lobbies.write().unwrap();

         // Fill empty slots
         for lobby in lobbies.values_mut().filter(|l| {
            l.game.is_none()
               && l.creation_time.elapsed() > Duration::from_secs(10)
               && (l.players.len() as u8) < l.max_players
               && l.password.is_empty()
         }) {
            let player_id = PlayerId(rand::random());
            let ai: Box<PalaceAi + Send + Sync> = Box::new(ai::random::new());
            add_player(
               Player {
                  name: ai::get_bot_name_clandestine(),
                  connection: Connection::Ai(AiState {
                     core: ai,
                     is_clandestine: true,
                  }),
                  turn_number: next_public_id(&lobby.players_by_turn_num),
               },
               player_id,
               lobby,
            );
         }

         // Create new lobbies
         if lobbies.len() < 5 {
            create_lobby(
               &mut lobbies,
               Connection::Ai(AiState {
                  core: Box::new(ai::random::new()),
                  is_clandestine: true,
               }),
               "botto grotto".into(),
               ai::get_bot_name_clandestine(),
               "".into(),
               4,
               data::default_turn_timer_secs(),
            );
         }

         // Start full lobbies that are owned by bots
         for lobby in lobbies
            .values_mut()
            .filter(|l| l.game.is_none() && l.players.len() as u8 == l.max_players && l.players[&l.owner].is_ai())
         {
            start_game(lobby);
         }
      });
   }

   ws::listen(address, |out| Server {
      out,
      lobbies: lobbies.clone(),
      connected_user: None,
   })
   .unwrap()
}
