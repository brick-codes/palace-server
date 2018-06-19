use game::{Card, PublicGameState};
use GameStartedEvent;

pub(crate) trait PalaceAi {
   fn player_name(&mut self) -> String {
      "BOT".into()
   }

   fn choose_faceup_cards(&mut self) -> (Card, Card, Card);

   fn take_turn(&mut self) -> [Card];

   fn on_game_state_update(&mut self, _new_state: &PublicGameState) {}

   fn on_game_start(&mut self, _game_start_event: GameStartedEvent) {}

   fn on_hand_update(&mut self, _new_hand: &[Card]) {}
}