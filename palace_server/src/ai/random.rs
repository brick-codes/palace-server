// This AI plays random cards
use super::PalaceAi;
use crate::data::GameStartEvent;
use crate::game::{Card, PublicGameState};
use rand::seq::SliceRandom;
use rand::{self, thread_rng};

pub struct RandomAi {
   hand: Vec<Card>,
   faceup_cards: Vec<Card>,
   turn_number: u8,
}

pub fn new() -> RandomAi {
   RandomAi {
      hand: vec![],
      faceup_cards: vec![],
      turn_number: 0,
   }
}

impl PalaceAi for RandomAi {
   fn strategy_name(&self) -> &'static str {
      "Random"
   }

   fn choose_three_faceup(&mut self) -> (Card, Card, Card) {
      (self.faceup_cards[0], self.faceup_cards[1], self.faceup_cards[2])
   }

   fn take_turn(&mut self) -> Box<[Card]> {
      if !self.hand.is_empty() {
         vec![*self.hand.choose(&mut thread_rng()).unwrap()].into_boxed_slice()
      } else {
         vec![*self.faceup_cards.choose(&mut thread_rng()).unwrap()].into_boxed_slice()
      }
   }

   fn on_game_state_update(&mut self, new_state: &PublicGameState) {
      self.faceup_cards.clear();
      self
         .faceup_cards
         .extend_from_slice(new_state.face_up_three[self.turn_number as usize]);
   }

   fn on_game_start(&mut self, game_start_event: GameStartEvent) {
      self.hand.extend_from_slice(game_start_event.hand);
      self.turn_number = game_start_event.turn_number;
   }

   fn on_hand_update(&mut self, new_hand: &[Card]) {
      self.hand.clear();
      self.hand.extend_from_slice(new_hand);
   }
}
