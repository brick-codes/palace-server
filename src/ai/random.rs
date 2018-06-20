// This AI plays random cards
use ai::PalaceAi;
use data::GameStartEvent;
use game::{Card, PublicGameState};
use rand::{self, Rng};

pub struct RandomAi {
   hand: Box<[Card]>,
   faceup_cards: Box<[Card]>,
   turn_number: u8,
}

pub fn new() -> RandomAi {
   RandomAi {
      hand: vec![].into_boxed_slice(),
      faceup_cards: vec![].into_boxed_slice(),
      turn_number: 0,
   }
}

impl PalaceAi for RandomAi {
   fn player_name(&mut self) -> String {
      "Randy".into()
   }

   fn choose_three_faceup(&mut self) -> (Card, Card, Card) {
      (self.faceup_cards[0], self.faceup_cards[1], self.faceup_cards[2])
   }

   fn take_turn(&mut self) -> Box<[Card]> {
      if !self.hand.is_empty() {
         vec![*rand::thread_rng().choose(&self.hand).unwrap()].into_boxed_slice()
      } else {
         vec![*rand::thread_rng().choose(&self.faceup_cards).unwrap()].into_boxed_slice()
      }
   }

   fn on_game_state_update(&mut self, new_state: &PublicGameState) {
      self.faceup_cards = new_state.face_up_three[self.turn_number as usize].into();
   }

   fn on_game_start(&mut self, game_start_event: GameStartEvent) {
      self.hand = game_start_event.hand.into();
      self.turn_number = game_start_event.turn_number;
   }

   fn on_hand_update(&mut self, new_hand: &[Card]) {
      self.hand = new_hand.into();
   }
}
