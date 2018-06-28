use std::collections::HashMap;

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
enum CardSuit {
   Clubs,
   Diamonds,
   Hearts,
   Spades,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd)]
enum CardValue {
   Two,
   Three,
   Four,
   Five,
   Six,
   Seven,
   Eight,
   Nine,
   Ten,
   Jack,
   Queen,
   King,
   Ace,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Card {
   value: CardValue,
   suit: CardSuit,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum GamePhase {
   Setup,
   Play,
   Complete,
}

#[derive(Debug, Deserialize)]
pub struct LobbyDisplay {
   pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct NewLobbyResponse {
   pub player_id: String,
   pub lobby_id: String,
}

#[derive(Debug, Deserialize)]
pub enum NewLobbyError {
   LessThanTwoMaxPlayers,
   EmptyLobbyName,
   EmptyPlayerName,
}

#[derive(Debug, Deserialize)]
pub struct PlayerJoinEvent {
   pub total_num_players: u8,
   pub new_player_name: String,
   pub slot: u8,
}

#[derive(Debug, Deserialize)]
pub struct PlayerLeaveEvent {
   pub total_num_players: u8,
   pub slot: u8,
}

#[derive(Debug, Deserialize)]
pub enum RequestAiError {
   NotLobbyOwner,
   LessThanOneAiRequested,
   LobbyNotFound,
   LobbyTooSmall,
   GameInProgress,
}

#[derive(Debug, Deserialize)]
pub enum KickPlayerError {
   NotLobbyOwner,
   LobbyNotFound,
   TargetPlayerNotFound,
   CantKickLobbyOwner,
}

#[derive(Debug, Deserialize)]
pub struct JoinLobbyResponse {
   pub player_id: String,
   pub lobby_players: Box<[String]>,
   pub max_players: u8,
}

#[derive(Debug, Deserialize)]
pub enum JoinLobbyError {
   LobbyNotFound,
   LobbyFull,
   BadPassword,
   GameInProgress,
   EmptyPlayerName,
}

#[derive(Debug, Deserialize, PartialEq)]
pub enum LobbyCloseEvent {
   Kicked,
   OwnerLeft,
   Afk,
}

#[derive(Debug, Deserialize)]
pub enum StartGameError {
   LobbyNotFound,
   NotLobbyOwner,
   LessThanTwoPlayers,
   GameInProgress,
}

#[derive(Debug, Deserialize)]
pub struct GameStartEvent {
   pub hand: Box<[Card]>,
   pub turn_number: u8,
   pub players: HashMap<u8, String>,
}

#[derive(Debug, Deserialize)]
pub struct PublicGameState {
   pub hands: Box<[u16]>,
   pub face_up_three: Box<[Box<[Card]>]>,
   pub face_down_three: Box<[u8]>,
   pub top_card: Option<Card>,
   pub pile_size: u16,
   pub cleared_size: u16,
   pub cur_phase: GamePhase,
   pub active_player: u8,
   pub last_cards_played: Box<[Card]>,
}

#[derive(Debug, Deserialize)]
pub enum InMessage {
   ListLobbiesResponse(Box<[LobbyDisplay]>),
   NewLobbyResponse(Result<NewLobbyResponse, NewLobbyError>),
   RequestAiResponse(Result<(), RequestAiError>),
   KickPlayerResponse(Result<(), KickPlayerError>),
   JoinLobbyResponse(Result<JoinLobbyResponse, JoinLobbyError>),
   StartGameResponse(Result<(), StartGameError>),
   PlayerJoinEvent(PlayerJoinEvent),
   PlayerLeaveEvent(PlayerLeaveEvent),
   LobbyCloseEvent(LobbyCloseEvent),
   GameStartEvent(GameStartEvent),
   PublicGameStateEvent(PublicGameState),
}

#[derive(Serialize)]
pub struct NewLobbyMessage<'a> {
   pub max_players: u8,
   pub password: &'a str,
   pub lobby_name: &'a str,
   pub player_name: &'a str,
}

#[derive(Serialize)]
pub struct RequestAiMessage<'a> {
   pub lobby_id: &'a str,
   pub player_id: &'a str,
   pub num_ai: u8,
}

#[derive(Serialize)]
pub struct KickPlayerMessage<'a> {
   pub lobby_id: &'a str,
   pub player_id: &'a str,
   pub slot: u8,
}

#[derive(Serialize)]
pub struct JoinLobbyMessage<'a> {
   pub lobby_id: &'a str,
   pub player_name: &'a str,
   pub password: &'a str,
}

#[derive(Serialize)]
pub struct StartGameMessage<'a> {
   pub lobby_id: &'a str,
   pub player_id: &'a str,
}

#[derive(Serialize)]
pub enum OutMessage<'a> {
   NewLobby(NewLobbyMessage<'a>),
   RequestAi(RequestAiMessage<'a>),
   KickPlayer(KickPlayerMessage<'a>),
   JoinLobby(JoinLobbyMessage<'a>),
   StartGame(StartGameMessage<'a>),
   ListLobbies,
}
