use crate::game::{Card, PublicGameState};
use crate::{LobbyDisplay, LobbyId, PlayerId};
use std::collections::HashMap;

use serde_derive::{Deserialize, Serialize};

pub fn default_turn_timer_secs() -> u8 {
   50
}

#[derive(Deserialize)]
pub struct NewLobbyMessage {
   pub max_players: u8,
   pub password: String,
   pub lobby_name: String,
   pub player_name: String,
   #[serde(default = "default_turn_timer_secs")]
   pub turn_timer: u8,
}

#[derive(Serialize)]
pub struct NewLobbyResponse {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
   pub max_players: u8,
}

#[derive(Debug, Serialize)]
pub enum NewLobbyError {
   LessThanTwoMaxPlayers,
   EmptyLobbyName,
   EmptyPlayerName,
   LobbyNameTooLong,
   PlayerNameTooLong,
   PasswordTooLong,
}

#[derive(Deserialize)]
pub struct JoinLobbyMessage {
   pub lobby_id: LobbyId,
   pub player_name: String,
   pub password: String,
}

#[derive(Serialize)]
pub struct JoinLobbyResponse<'a> {
   pub player_id: PlayerId,
   pub lobby_players: Vec<&'a str>,
   pub max_players: u8,
   pub num_spectators: u8,
   pub turn_timer: u8,
}

#[derive(Serialize)]
pub enum JoinLobbyError {
   LobbyNotFound,
   LobbyFull,
   BadPassword,
   GameInProgress,
   EmptyPlayerName,
   PlayerNameTooLong,
}

#[derive(Serialize)]
pub struct SpectateLobbyResponse<'a> {
   pub lobby_players: Vec<&'a str>,
   pub max_players: u8,
   pub num_spectators: u8,
   pub turn_timer: u8,
}

#[derive(Serialize)]
pub enum SpectateLobbyError {
   LobbyNotFound,
   SpectateLobbyFull,
}

#[derive(Copy, Clone, Deserialize)]
pub struct StartGameMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
}

#[derive(Copy, Clone, Deserialize)]
pub struct ChooseFaceupMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub card_one: Card,
   pub card_two: Card,
   pub card_three: Card,
}

#[derive(Deserialize)]
pub struct MakePlayMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub cards: Box<[Card]>,
}

#[derive(Serialize)]
pub struct GameStartEvent<'a> {
   pub hand: &'a [Card],
   pub turn_number: u8,
   pub players: &'a HashMap<u8, String>,
}

#[derive(Serialize)]
pub struct SpectateGameStartEvent<'a> {
   pub players: &'a HashMap<u8, String>,
}

#[derive(Serialize)]
pub enum StartGameError {
   LobbyNotFound,
   NotLobbyOwner,
   LessThanTwoPlayers,
   GameInProgress,
}

#[derive(Deserialize)]
pub struct ReconnectMessage {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
}

#[derive(Serialize)]
pub struct ReconnectResponse {
   pub max_players: u8,
   pub num_spectators: u8,
   pub turn_timer: u8,
}

#[derive(Deserialize)]
pub struct KickPlayerMessage {
   pub player_id: PlayerId,
   pub lobby_id: LobbyId,
   pub slot: u8,
}

#[derive(Serialize)]
pub enum MakePlayError {
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
   GameError(&'static str),
}

#[derive(Serialize)]
pub enum ChooseFaceupError {
   LobbyNotFound,
   GameNotStarted,
   PlayerNotFound,
   NotYourTurn,
   GameError(&'static str),
}

#[derive(Serialize)]
pub enum ReconnectError {
   LobbyNotFound,
   PlayerNotFound,
   PlayerKicked,
}

#[derive(Serialize)]
pub struct PlayerJoinEvent<'a> {
   pub total_num_players: u8,
   pub new_player_name: &'a str,
   pub slot: u8,
}

#[derive(Serialize)]
pub struct PlayerLeaveEvent {
   pub total_num_players: u8,
   pub slot: u8,
}

#[derive(Copy, Clone, Deserialize)]
pub struct RequestAiMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub num_ai: u8,
}

#[derive(Serialize)]
pub enum RequestAiError {
   NotLobbyOwner,
   LessThanOneAiRequested,
   LobbyNotFound,
   LobbyTooSmall,
   GameInProgress,
}

#[derive(Serialize)]
pub enum KickPlayerError {
   NotLobbyOwner,
   LobbyNotFound,
   TargetPlayerNotFound,
   CantKickLobbyOwner,
   CantKickAiDuringGame,
}

#[derive(Serialize)]
pub enum LobbyCloseEvent {
   Kicked,
   OwnerLeft,
   Afk,
}

#[derive(Deserialize)]
pub struct ListLobbiesMessage {
   pub page: u64,
}

#[derive(Deserialize)]
pub enum PalaceInMessage {
   NewLobby(NewLobbyMessage),
   JoinLobby(JoinLobbyMessage),
   ListLobbies(ListLobbiesMessage),
   StartGame(StartGameMessage),
   ChooseFaceup(ChooseFaceupMessage),
   MakePlay(MakePlayMessage),
   Reconnect(ReconnectMessage),
   RequestAi(RequestAiMessage),
   KickPlayer(KickPlayerMessage),
   SpectateLobby(LobbyId),
}

#[derive(Serialize)]
pub struct ListLobbyResponse<'a> {
   pub lobbies: &'a [LobbyDisplay<'a>],
   pub has_next_page: bool,
}

#[derive(Serialize)]
pub enum PalaceOutMessage<'a> {
   NewLobbyResponse(Result<NewLobbyResponse, NewLobbyError>),
   JoinLobbyResponse(Result<JoinLobbyResponse<'a>, JoinLobbyError>),
   ListLobbiesResponse(ListLobbyResponse<'a>),
   StartGameResponse(Result<(), StartGameError>),
   ChooseFaceupResponse(Result<(), ChooseFaceupError>),
   MakePlayResponse(Result<(), MakePlayError>),
   ReconnectResponse(Result<ReconnectResponse, ReconnectError>),
   RequestAiResponse(Result<(), RequestAiError>),
   KickPlayerResponse(Result<(), KickPlayerError>),
   SpectateLobbyResponse(Result<SpectateLobbyResponse<'a>, SpectateLobbyError>),
   PublicGameStateEvent(&'a PublicGameState<'a>),
   HandEvent(&'a [Card]),
   GameStartEvent(GameStartEvent<'a>),
   SpectateGameStartEvent(SpectateGameStartEvent<'a>),
   PlayerJoinEvent(PlayerJoinEvent<'a>),
   PlayerLeaveEvent(PlayerLeaveEvent),
   LobbyCloseEvent(LobbyCloseEvent),
   SpectatorJoinEvent(()),
   SpectatorLeaveEvent(()),
   GameCompleteEvent(&'a [u8]),
}
