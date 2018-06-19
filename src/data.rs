use game::{Card, PublicGameState};
use {Lobby, LobbyId, PlayerId};

impl Lobby {
   pub(crate) fn display(&self, lobby_id: &LobbyId) -> LobbyDisplay {
      LobbyDisplay {
         cur_players: self.players.len() as u8,
         ai_players: self.ai_players.len() as u8,
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
}

#[derive(Serialize)]
pub(crate) enum JoinLobbyError {
   LobbyNotFound,
   LobbyFull,
   BadPassword,
   GameInProgress,
   EmptyPlayerName,
}

#[derive(Deserialize)]
pub(crate) struct StartGameMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
}

#[derive(Deserialize)]
pub(crate) struct ChooseFaceupMessage {
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
   pub card_one: Card,
   pub card_two: Card,
   pub card_three: Card,
}

#[derive(Deserialize)]
pub(crate) struct MakePlayMessage {
   pub cards: Box<[Card]>,
   pub lobby_id: LobbyId,
   pub player_id: PlayerId,
}

#[derive(Serialize)]
pub(crate) struct HandResponse<'a> {
   pub hand: &'a [Card],
}

#[derive(Serialize)]
pub(crate) struct GameStartedEvent<'a> {
   pub hand: &'a [Card],
   pub turn_number: u8,
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
}

#[derive(Serialize)]
pub(crate) struct PlayerJoinEvent<'a> {
   pub total_num_players: u8,
   pub new_player_name: &'a str,
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
}

#[derive(Serialize)]
pub(crate) enum PalaceOutMessage<'a> {
   NewLobbyResponse(Result<NewLobbyResponse, NewLobbyError>),
   JoinLobbyResponse(Result<JoinLobbyResponse<'a>, JoinLobbyError>),
   ListLobbiesResponse(&'a [LobbyDisplay<'a>]),
   StartGameResponse(Result<(), StartGameError>),
   ChooseFaceupResponse(Result<HandResponse<'a>, ChooseFaceupError>),
   MakePlayResponse(Result<HandResponse<'a>, MakePlayError>),
   ReconnectResponse(Result<(), ReconnectError>),
   PublicGameStateEvent(&'a PublicGameState<'a>),
   GameStartedEvent(GameStartedEvent<'a>),
   PlayerJoinEvent(PlayerJoinEvent<'a>),
}
