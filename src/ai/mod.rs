use game::{Card, GamePhase};
use serde_json;
use std;
use ws::{self, CloseCode, Handler, Handshake, Message, Sender};
use ChooseFaceupError;
use JoinLobbyError;
use JoinLobbyResponse;
use LobbyId;
use MakePlayError;
use PlayerId;
use ChooseFaceupMessage;

trait PalaceAi {
   fn player_name(&mut self) -> String {
      "BOT".into()
   }

   fn choose_faceup_cards(&mut self) -> (Card, Card, Card);

   fn take_turn(&mut self) -> Box<[Card]>;

   fn on_game_state_update(&mut self, _new_state: &PublicGameState) {}

   fn on_game_start(&mut self, _game_start_event: &GameStarted) {}

   fn on_hand_update(&mut self, _new_hand: Box<[Card]>) {}
}

fn spawn_and_join<P>(lobby_id: LobbyId, password: String, ai: P)
where
   P: PalaceAi + Send + Clone + 'static,
{
   std::thread::spawn(move || {
      ws::connect("ws://127.0.0.1:3012", move |out| AiClient {
         out,
         lobby_id,
         password: password.clone(),
         ai: ai.clone(),
         player_number: None,
         player_id: None,
      }).unwrap();
   });
}

#[derive(Serialize)]
struct JoinLobby<'a> {
   name: &'a str,
   lobby_id: LobbyId,
   password: &'a str,
}

#[derive(Serialize)]
struct MakePlay<'a> {
   cards: &'a [Card],
   lobby_id: LobbyId,
   player_id: PlayerId,
}

#[derive(Deserialize)]
enum AiInMessage {
   PublicGameStateEvent(PublicGameState),
   GameStartedEvent(GameStarted),
   MakePlayResponse(Result<HandResponse, MakePlayError>),
   ChooseFaceupResponse(Result<HandResponse, ChooseFaceupError>),
   JoinLobbyResponse(Result<JoinLobbyResponse, JoinLobbyError>),
}

#[derive(Serialize)]
enum AiOutMessage<'a> {
   JoinLobby(JoinLobby<'a>),
   MakePlay(MakePlay<'a>),
   ChooseFaceup(ChooseFaceupMessage),
}

struct AiClient<P>
where
   P: PalaceAi,
{
   out: Sender,
   lobby_id: LobbyId,
   password: String,
   ai: P,
   player_number: Option<u8>,
   player_id: Option<PlayerId>,
}

impl<P> Handler for AiClient<P>
where
   P: PalaceAi,
{
   fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
      let result = self.out.send(
         serde_json::to_vec(&AiOutMessage::JoinLobby(JoinLobby {
            name: &self.ai.player_name(),
            lobby_id: self.lobby_id,
            password: &self.password,
         })).unwrap(),
      );

      self.password.clear();
      self.password.shrink_to_fit();

      result
   }

   fn on_message(&mut self, msg: Message) -> ws::Result<()> {
      match msg {
         Message::Text(_) => self.out.close(CloseCode::Unsupported),
         Message::Binary(binary) => match serde_json::from_slice::<AiInMessage>(&binary) {
            Ok(message) => {
               match message {
                  AiInMessage::PublicGameStateEvent(pgs) => {
                     self.ai.on_game_state_update(&pgs);
                     if self
                        .player_number
                        .expect("Got public game state event before AI registered game start")
                        == pgs.active_player
                     {
                        match pgs.cur_phase {
                           GamePhase::Complete => {}
                           GamePhase::Play => {
                              self.ai.take_turn();
                           }
                           GamePhase::Setup => {
                              self.ai.choose_faceup_cards();
                           }
                        }
                     }
                  }
                  AiInMessage::GameStartedEvent(gse) => {
                     self.player_number = Some(gse.turn_number);
                     self.ai.on_game_start(&gse);
                  }
                  AiInMessage::JoinLobbyResponse(jlr) => {
                     self.player_id = Some(jlr.expect("AI failed to join game").player_id);
                  }
                  AiInMessage::MakePlayResponse(mpr) => {
                     self.ai.on_hand_update(mpr.expect("AI failed to make play").hand);
                  }
                  AiInMessage::ChooseFaceupResponse(cfr) => {
                      self.ai.on_hand_update(cfr.expect("AI failed to choose faceup cards").hand);
                  }
               }
               Ok(())
            }
            Err(e) => Ok(()),
         },
      }
   }
}

#[derive(Deserialize)]
struct GameStarted {
   hand: Box<[Card]>,
   turn_number: u8,
}

#[derive(Deserialize)]
struct HandResponse {
   hand: Box<[Card]>,
}

#[derive(Deserialize)]
pub struct PublicGameState {
   hands: Box<[usize]>,
   face_up_three: Box<[Box<[Card]>]>,
   face_down_three: Box<[u8]>,
   top_card: Option<Card>,
   pile_size: usize,
   cleared_size: usize,
   cur_phase: GamePhase,
   active_player: u8,
   last_cards_played: Box<[Card]>,
}
