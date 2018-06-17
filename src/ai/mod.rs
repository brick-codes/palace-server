use game::{Card, GamePhase};
use std;
use std::sync::mpsc;
use ws::{self, Handler, Handshake, Sender, WebSocket};
use LobbyId;
use serde_json;

trait PalaceAi {
   fn player_name(&mut self) -> String {
      "BOT".into()
   }

   fn choose_faceup_cards(&mut self) -> [Card; 3];

   fn take_turn(&mut self) -> Box<[Card]>;

   fn on_game_state_update(&mut self, state: PublicGameState) {}
}

fn spawn_and_join<P>(lobby_id: LobbyId, password: String, ai: P) where P: PalaceAi + Send + Clone + 'static {
   std::thread::spawn(move ||{
      ws::connect("ws://127.0.0.1:3012", move |out| AiClient {
         out,
         lobby_id,
         password: password.clone(),
         ai: ai.clone(),
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
enum AiOutMessage<'a> {
   JoinLobby(JoinLobby<'a>),
   ListLobbies,
}

struct AiClient<P> where P: PalaceAi {
   out: Sender,
   lobby_id: LobbyId,
   password: String,
   ai: P,
}

impl<P> Handler for AiClient<P> where P: PalaceAi {
   fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
      let result = self.out.send(serde_json::to_vec(&AiOutMessage::JoinLobby(JoinLobby {
         name: &self.ai.player_name(),
         lobby_id: self.lobby_id,
         password: &self.password,
      })).unwrap());

      self.password.clear();
      self.password.shrink_to_fit();

      result
   }
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
