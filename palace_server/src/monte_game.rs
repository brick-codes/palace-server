use crate::game::{
   is_playable_without_pickup, new_deck, top_n_cards_same, Card, CardValue, CardZone, Phase, HAND_SIZE,
};
use rand::seq::SliceRandom;
use rand::thread_rng;

#[derive(Clone, Debug)]
pub struct GameState {
   pub active_player: u8,
   pub num_players: u8,
   pub hands: Box<[Vec<Card>]>,
   pub face_up_three: Box<[Vec<Card>]>,
   pub face_down_three: Box<[Vec<Card>]>,
   pub pile_cards: Vec<Card>,
   pub cur_phase: Phase,
   pub out_players: Vec<u8>,
}

impl GameState {
   pub fn new(num_players: u8) -> GameState {
      let mut deck: Vec<Card> = new_deck(num_players as usize).collect();
      deck.shuffle(&mut thread_rng());
      let mut deck = deck.into_iter();
      let mut face_down_three = Vec::with_capacity(num_players as usize);
      let mut face_up_three = Vec::with_capacity(num_players as usize);
      let mut hands = Vec::with_capacity(num_players as usize);
      for _ in 0..num_players {
         face_up_three.push(deck.by_ref().take(3).collect());
         face_down_three.push(deck.by_ref().take(3).collect());
         hands.push(deck.by_ref().take(HAND_SIZE).collect());
      }
      GameState {
         active_player: 0,
         num_players,
         hands: hands.into_boxed_slice(),
         face_down_three: face_down_three.into_boxed_slice(),
         face_up_three: face_up_three.into_boxed_slice(),
         pile_cards: Vec::new(),
         cur_phase: Phase::Setup,
         out_players: Vec::with_capacity(num_players as usize),
      }
   }

   /// Return bool = whether or not the game is complete
   pub fn take_turn(&mut self, cards: &[Card]) -> Result<bool, &'static str> {
      match self.cur_phase {
         Phase::Setup => {
            self.choose_three_faceup(cards[0], cards[1], cards[2])?;
            Ok(false)
         }
         Phase::Play => self.make_play(cards),
      }
   }

   fn choose_three_faceup(&mut self, card_one: Card, card_two: Card, card_three: Card) -> Result<(), &'static str> {
      // Combine hand + face up cards
      let mut all_cards = self.hands[self.active_player as usize].clone();
      all_cards.extend_from_slice(&self.face_up_three[self.active_player as usize]);

      // Calculate the new hand after removing new face up cards
      let mut card_one_removed = false;
      let mut card_two_removed = false;
      let mut card_three_removed = false;
      let mut new_hand = Vec::with_capacity(HAND_SIZE);
      for card in all_cards {
         if card == card_one && !card_one_removed {
            card_one_removed = true;
         } else if card == card_two && !card_two_removed {
            card_two_removed = true;
         } else if card == card_three && !card_three_removed {
            card_three_removed = true;
         } else {
            new_hand.push(card);
         }
      }

      // Mutate state
      self.face_up_three[self.active_player as usize] = vec![card_one, card_two, card_three];
      self.face_up_three[self.active_player as usize].sort_unstable();
      self.hands[self.active_player as usize] = new_hand;
      self.hands[self.active_player as usize].sort_unstable();
      self.rotate_play();

      if self.active_player == 0 {
         self.cur_phase = Phase::Play;
      }

      Ok(())
   }

   /// Return bool = whether or not the game is complete
   fn make_play(&mut self, cards: &[Card]) -> Result<bool, &'static str> {
      // Figure out which zone we are retrieving cards from
      let hand_len = self.hands[self.active_player as usize].len();
      let fup3_len = self.face_up_three[self.active_player as usize].len();
      let a_card;
      let (card_zone, cards) = if hand_len > 0 {
         (CardZone::Hand, cards)
      } else if fup3_len > 0 {
         (CardZone::FaceUpThree, cards)
      } else {
         // In the case of face down cards, we can safely pop now as there's no way this play can fail
         a_card = [self.face_down_three[self.active_player as usize].pop().unwrap()];
         (CardZone::FaceDownThree, a_card.as_ref())
      };

      // check that play is valid
      let play_value = cards[0].value;

      // Remove cards from old zone
      match card_zone {
         CardZone::Hand => {
            for card in cards.iter() {
               let i = self.hands[self.active_player as usize].binary_search(card).unwrap();
               self.hands[self.active_player as usize].remove(i);
            }
         }
         CardZone::FaceUpThree => {
            for card in cards.iter() {
               let i = self.face_up_three[self.active_player as usize]
                  .binary_search(card)
                  .unwrap();
               self.face_up_three[self.active_player as usize].remove(i);
            }
         }
         CardZone::FaceDownThree => {
            // Already popped
         }
      }

      let is_playable = is_playable_without_pickup(play_value, &self.pile_cards);

      // Put cards in pile
      self.pile_cards.extend_from_slice(cards);

      let player_out = if !is_playable {
         self.hands[self.active_player as usize].extend_from_slice(&self.pile_cards);
         self.hands[self.active_player as usize].sort_unstable();
         self.pile_cards.clear();
         false
      } else if self.hands[self.active_player as usize].is_empty()
         && self.face_up_three[self.active_player as usize].is_empty()
         && self.face_down_three[self.active_player as usize].is_empty()
      {
         self.out_players.push(self.active_player);
         if self.out_players.len() as u8 == self.num_players - 1 {
            self.out_players.push(self.next_player());
            return Ok(true);
         }
         true
      } else {
         false
      };

      if (is_playable && play_value == CardValue::Ten) || top_n_cards_same(&self.pile_cards, self.num_players as usize)
      {
         self.pile_cards.clear();
         if player_out {
            self.rotate_play();
         }
      } else {
         self.rotate_play();
      }

      Ok(false)
   }

   pub fn get_hand(&self, player_num: u8) -> &[Card] {
      &self.hands[player_num as usize]
   }

   fn next_player(&self) -> u8 {
      let mut next_player = self.active_player + 1;
      while self.out_players.contains(&next_player) {
         next_player += 1;
      }
      if next_player == self.num_players {
         next_player = 0;
      }
      while self.out_players.contains(&next_player) {
         next_player += 1;
      }
      next_player
   }

   fn rotate_play(&mut self) {
      self.active_player = self.next_player();
   }
}
