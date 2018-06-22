use data::GameStartEvent;
use game::{Card, PublicGameState};

pub mod random;

pub(crate) trait PalaceAi {
   // Could be associated const if we stop using trait object
   fn strategy_name(&self) -> &'static str;

   fn choose_three_faceup(&mut self) -> (Card, Card, Card);

   fn take_turn(&mut self) -> Box<[Card]>;

   fn on_game_state_update(&mut self, _new_state: &PublicGameState) {}

   fn on_game_start(&mut self, _game_start_event: GameStartEvent) {}

   fn on_hand_update(&mut self, _new_hand: &[Card]) {}
}
