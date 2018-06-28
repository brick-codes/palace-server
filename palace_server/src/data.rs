use game::{Card, PublicGameState};
use std::collections::HashMap;
use {Lobby, LobbyId, PlayerId};

impl Lobby {
   pub(crate) fn display(&self, lobby_id: &LobbyId) -> LobbyDisplay {
      LobbyDisplay {
         cur_players: self.players.len() as u8,
         ai_players: self.players.values().filter(|p| p.is_ai()).count() as u8,
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
}

#[derive(Deserialize)]
pub(crate) struct NewLobbyMessage {
   pub max_players: u8,
   pub password: String,
   pub lobby_name: String,
   pub player_name: String,
}

#[derive(Serialize)]
pub(crate) struct NewLobbyResponse {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
   pub max_players: u8,
}

#[derive(Debug, Serialize)]
pub(crate) enum NewLobbyError {
   LessThanTwoMaxPlayers,
   EmptyLobbyName,
   EmptyPlayerName,
}

#[derive(Deserialize)]
pub(crate) struct JoinLobbyMessage {
   pub lobby_id: LobbyId,
   pub player_name: String,
   pub password: String,
}

#[derive(Serialize)]
pub(crate) struct JoinLobbyResponse<'a> {
   pub player_id: PlayerId,
   pub lobby_players: Vec<&'a str>,
   pub max_players: u8,
}

#[derive(Serialize)]
pub(crate) enum JoinLobbyError {
   LobbyNotFound,
   LobbyFull,
   BadPassword,
   GameInProgress,
   EmptyPlayerName,
}

#[derive(Copy, Clone, Deserialize)]
pub(crate) struct StartGameMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
}

#[derive(Copy, Clone, Deserialize)]
pub(crate) struct ChooseFaceupMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub card_one: Card,
   pub card_two: Card,
   pub card_three: Card,
}

#[derive(Deserialize)]
pub(crate) struct MakePlayMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub cards: Box<[Card]>,
}

#[derive(Serialize)]
pub(crate) struct GameStartEvent<'a> {
   pub hand: &'a [Card],
   pub turn_number: u8,
   pub players: &'a HashMap<u8, String>,
}

#[derive(Serialize)]
pub(crate) enum StartGameError {
   LobbyNotFound,
   NotLobbyOwner,
   LessThanTwoPlayers,
   GameInProgress,
}

#[derive(Deserialize)]
pub(crate) struct ReconnectMessage {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
}

#[derive(Deserialize)]
pub(crate) struct KickPlayerMessage {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
   pub slot: u8,
}

#[derive(Serialize)]
pub(crate) enum MakePlayError {
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
   GameError(&'static str),
}

#[derive(Serialize)]
pub(crate) enum ChooseFaceupError {
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
   GameError(&'static str),
}

#[derive(Serialize)]
pub(crate) enum ReconnectError {
   LobbyNotFound,
   PlayerNotFound,
   PlayerKicked,
}

#[derive(Serialize)]
pub(crate) struct PlayerJoinEvent<'a> {
   pub total_num_players: u8,
   pub new_player_name: &'a str,
   pub slot: u8,
}

#[derive(Serialize)]
pub(crate) struct PlayerLeaveEvent {
   pub total_num_players: u8,
   pub slot: u8,
}

#[derive(Copy, Clone, Deserialize)]
pub(crate) struct RequestAiMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub num_ai: u8,
}

#[derive(Serialize)]
pub(crate) enum RequestAiError {
   NotLobbyOwner,
   LessThanOneAiRequested,
   LobbyNotFound,
   LobbyTooSmall,
   GameInProgress,
}

#[derive(Serialize)]
pub(crate) enum KickPlayerError {
   NotLobbyOwner,
   LobbyNotFound,
   TargetPlayerNotFound,
   CantKickLobbyOwner,
   GameInProgress,
}

#[derive(Serialize)]
pub(crate) enum LobbyCloseEvent {
   Kicked,
   OwnerLeft,
   Afk,
}

#[derive(Deserialize)]
pub(crate) enum PalaceInMessage {
   NewLobby(NewLobbyMessage),
   JoinLobby(JoinLobbyMessage),
   ListLobbies,
   StartGame(StartGameMessage),
   ChooseFaceup(ChooseFaceupMessage),
   MakePlay(MakePlayMessage),
   Reconnect(ReconnectMessage),
   RequestAi(RequestAiMessage),
   KickPlayer(KickPlayerMessage),
}

#[derive(Serialize)]
pub(crate) enum PalaceOutMessage<'a> {
   NewLobbyResponse(Result<NewLobbyResponse, NewLobbyError>),
   JoinLobbyResponse(Result<JoinLobbyResponse<'a>, JoinLobbyError>),
   ListLobbiesResponse(&'a [LobbyDisplay<'a>]),
   StartGameResponse(Result<(), StartGameError>),
   ChooseFaceupResponse(Result<(), ChooseFaceupError>),
   MakePlayResponse(Result<(), MakePlayError>),
   ReconnectResponse(Result<(), ReconnectError>),
   RequestAiResponse(Result<(), RequestAiError>),
   KickPlayerResponse(Result<(), KickPlayerError>),
   PublicGameStateEvent(&'a PublicGameState<'a>),
   HandEvent(&'a [Card]),
   GameStartEvent(GameStartEvent<'a>),
   PlayerJoinEvent(PlayerJoinEvent<'a>),
   PlayerLeaveEvent(PlayerLeaveEvent),
   LobbyCloseEvent(LobbyCloseEvent),
}
