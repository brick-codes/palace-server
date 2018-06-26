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
   pub lobby_players: Vec<String>,
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
}

#[derive(Debug, Deserialize)]
pub enum InMessage {
   ListLobbiesResponse(Box<[LobbyDisplay]>),
   NewLobbyResponse(Result<NewLobbyResponse, NewLobbyError>),
   RequestAiResponse(Result<(), RequestAiError>),
   KickPlayerResponse(Result<(), KickPlayerError>),
   JoinLobbyResponse(Result<JoinLobbyResponse, JoinLobbyError>),
   PlayerJoinEvent(PlayerJoinEvent),
   PlayerLeaveEvent(PlayerLeaveEvent),
   LobbyCloseEvent(LobbyCloseEvent),
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
pub enum OutMessage<'a> {
   NewLobby(NewLobbyMessage<'a>),
   RequestAi(RequestAiMessage<'a>),
   KickPlayer(KickPlayerMessage<'a>),
   JoinLobby(JoinLobbyMessage<'a>),
   ListLobbies,
}
