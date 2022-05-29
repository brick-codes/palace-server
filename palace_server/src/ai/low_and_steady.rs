// This AI plays cards following simple rules (lowest card we can)
use super::PalaceAi;
use crate::data::GameStartEvent;
use crate::game::{is_playable_without_pickup, Card, CardValue, PublicGameState};

pub struct LowAndSteadyAi {
   hand: Vec<Card>,
   faceup_cards: Vec<Card>,
   turn_number: u8,
   cur_pile: Vec<Card>,
}

pub fn new() -> LowAndSteadyAi {
   LowAndSteadyAi {
      hand: vec![],
      faceup_cards: vec![],
      turn_number: 0,
      cur_pile: vec![],
   }
}

fn loewst_playable_cards(card_zone: &[Card], pile: &[Card]) -> Box<[Card]> {
   for i in 0..card_zone.len() {
      if is_playable_without_pickup(card_zone[i].value, pile) {
         let mut j = i + 1;
         while let Some(c) = card_zone.get(j) {
            if c.value == card_zone[i].value {
               j += 1;
            } else {
               break;
            }
         }
         return card_zone[i..j].to_vec().into_boxed_slice();
      }
   }
   vec![*card_zone.first().unwrap()].into_boxed_slice()
}

fn sort_zone_low_to_high_specials_in_back(card_zone: &mut [Card]) {
   card_zone.sort_unstable_by(|x, y| {
      if x.value == CardValue::Four {
         std::cmp::Ordering::Greater
      } else if y.value == CardValue::Four {
         std::cmp::Ordering::Less
      } else if x.value == CardValue::Two {
         std::cmp::Ordering::Greater
      } else if y.value == CardValue::Two {
         std::cmp::Ordering::Less
      } else if x.value == CardValue::Ten {
         std::cmp::Ordering::Greater
      } else if y.value == CardValue::Ten {
         std::cmp::Ordering::Less
      } else {
         x.cmp(y)
      }
   });
}

impl PalaceAi for LowAndSteadyAi {
   fn strategy_name(&self) -> &'static str {
      "Low and Steady"
   }

   fn choose_three_faceup(&mut self) -> Box<[Card]> {
      self.faceup_cards.clone().into_boxed_slice()
   }

   fn make_play(&mut self) -> Box<[Card]> {
      if !self.hand.is_empty() {
         loewst_playable_cards(&self.hand, &self.cur_pile)
      } else {
         sort_zone_low_to_high_specials_in_back(&mut self.faceup_cards);
         loewst_playable_cards(&self.faceup_cards, &self.cur_pile)
      }
   }

   fn on_game_state_update(&mut self, new_state: &PublicGameState) {
      self.faceup_cards.clear();
      self
         .faceup_cards
         .extend_from_slice(new_state.face_up_three[self.turn_number as usize]);
      if new_state.pile_size == 0 {
         self.cur_pile.clear();
      } else {
         self.cur_pile.extend_from_slice(new_state.last_cards_played);
      }
   }

   fn on_game_start(&mut self, game_start_event: GameStartEvent) {
      self.hand.extend_from_slice(game_start_event.hand);
      self.turn_number = game_start_event.turn_number;
   }

   fn on_hand_update(&mut self, new_hand: &[Card]) {
      self.hand.clear();
      self.hand.extend_from_slice(new_hand);
      sort_zone_low_to_high_specials_in_back(&mut self.hand);
   }
}
