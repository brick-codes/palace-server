use rand::seq::SliceRandom;
use rand::thread_rng;
use serde_derive::{Deserialize, Serialize};
use std::time::Instant;

pub const HAND_SIZE: usize = 6;

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum CardSuit {
   Clubs,
   Diamonds,
   Hearts,
   Spades,
}

const SUITS: [CardSuit; 4] = [CardSuit::Clubs, CardSuit::Diamonds, CardSuit::Hearts, CardSuit::Spades];

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum CardValue {
   Two,
   Three,
   Four,
   Five,
   Six,
   Seven,
   Eight,
   Nine,
   Ten,
   Jack,
   Queen,
   King,
   Ace,
}

const VALUES: [CardValue; 13] = [
   CardValue::Two,
   CardValue::Three,
   CardValue::Four,
   CardValue::Five,
   CardValue::Six,
   CardValue::Seven,
   CardValue::Eight,
   CardValue::Nine,
   CardValue::Ten,
   CardValue::Jack,
   CardValue::Queen,
   CardValue::King,
   CardValue::Ace,
];

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct Card {
   pub value: CardValue,
   pub suit: CardSuit,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Phase {
   Setup,
   Play,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum CardZone {
   Hand,
   FaceUpThree,
   FaceDownThree,
}

#[derive(Clone, Debug)]
pub struct GameState {
   pub active_player: u8,
   pub num_players: u8,
   pub hands: Box<[Vec<Card>]>,
   pub face_up_three: Box<[Vec<Card>]>,
   pub face_down_three: Box<[Vec<Card>]>,
   pub cleared_cards: Vec<Card>,
   pub pile_cards: Vec<Card>,
   pub cur_phase: Phase,
   pub last_cards_played: Vec<Card>,
   pub out_players: Vec<u8>,
   pub last_turn_start: Instant,
   pub last_played_zone: Option<CardZone>,
}

pub fn new_deck(num_players: usize) -> impl Iterator<Item = Card> {
   VALUES
      .iter()
      .cycle()
      .take(num_players * VALUES.len())
      .zip(SUITS.iter().take(num_players).cycle())
      .map(|(value, suit)| Card {
         suit: *suit,
         value: *value,
      })
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
         cleared_cards: Vec::new(),
         pile_cards: Vec::new(),
         cur_phase: Phase::Setup,
         last_cards_played: Vec::new(),
         out_players: Vec::with_capacity(num_players as usize),
         last_turn_start: Instant::now(),
         last_played_zone: None,
      }
   }

   pub fn public_state(&self) -> PublicGameState {
      PublicGameState {
         hands: self
            .hands
            .iter()
            .map(|x| x.len() as u16)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
         face_up_three: self
            .face_up_three
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
         face_down_three: self
            .face_down_three
            .iter()
            .map(|x| x.len() as u8)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
         top_card: self.pile_cards.last().cloned(),
         pile_size: self.pile_cards.len() as u16,
         cleared_size: self.cleared_cards.len() as u16,
         cur_phase: self.cur_phase,
         active_player: self.active_player,
         last_cards_played: &self.last_cards_played,
         last_played_zone: self.last_played_zone,
      }
   }

   /// Return bool = whether or not the game is complete
   pub fn take_turn(&mut self, cards: &[Card]) -> Result<bool, &'static str> {
      match self.cur_phase {
         Phase::Setup => {
            if cards.len() != 3 {
               return Err("During setup, must choose exactly three cards");
            }
            self.choose_three_faceup(cards[0], cards[1], cards[2])?;
            Ok(false)
         }
         Phase::Play => {
            self.make_play(cards)
         },
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

      // Make sure that all three cards put face up were removed from hand
      if !(card_one_removed && card_two_removed && card_three_removed) {
         return Err("Chosen three faceup cards are not in hand / already faceup cards");
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
         if cards.len() > hand_len {
            return Err("Can't play more cards than you have");
         }
         (CardZone::Hand, cards)
      } else if fup3_len > 0 {
         if cards.len() > fup3_len {
            return Err("Can't play more cards than you have");
         }
         (CardZone::FaceUpThree, cards)
      } else {
         if !cards.is_empty() {
            return Err("Can't choose any cards when playing from the face down three");
         }
         // In the case of face down cards, we can safely pop now as there's no way this play can fail
         a_card = [self.face_down_three[self.active_player as usize].pop().unwrap()];
         (CardZone::FaceDownThree, a_card.as_ref())
      };

      if cards.is_empty() {
         return Err("Have to play at least one card");
      }

      // check that play is valid
      let play_value = cards[0].value;

      for card in cards {
         if card.value != play_value {
            return Err("Can only play multiple cards if each card has the same value");
         }

         match card_zone {
            CardZone::Hand => {
               if self.hands[self.active_player as usize].binary_search(card).is_err() {
                  return Err("can only play cards that you have");
               }
            }
            CardZone::FaceUpThree => {
               if self.face_up_three[self.active_player as usize]
                  .binary_search(card)
                  .is_err()
               {
                  return Err("can only play cards that you have");
               }
            }
            CardZone::FaceDownThree => {
               // already checked
            }
         }
      }

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

      self.last_played_zone = Some(card_zone);

      let is_playable = is_playable_without_pickup(play_value, &self.pile_cards);

      // Put cards in pile
      self.pile_cards.extend_from_slice(&cards);

      self.last_cards_played.clear();
      self.last_cards_played.extend_from_slice(&cards);

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

      if (is_playable && play_value == CardValue::Ten) || self.top_n_cards_same() {
         self.cleared_cards.extend_from_slice(&self.pile_cards);
         self.pile_cards.clear();
         if player_out {
            self.rotate_play();
         }
      } else {
         self.rotate_play();
      }

      Ok(false)
   }

   fn top_n_cards_same(&self) -> bool {
      let top_value = if let Some(card) = self.pile_cards.last() {
         card.value
      } else {
         return false;
      };
      let mut top_n_same = 0;
      for card in self.pile_cards.iter().rev() {
         if card.value == top_value {
            top_n_same += 1;
         } else if card.value == CardValue::Four {
            continue;
         } else {
            break;
         }
      }
      top_n_same == self.num_players
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
      self.last_turn_start = Instant::now();
   }
}

pub fn is_playable_without_pickup(card_value: CardValue, pile: &[Card]) -> bool {
   match (card_value, effective_top_card(pile)) {
      (CardValue::Two, _) => true,
      (CardValue::Four, _) => true,
      (CardValue::Ten, y) => y != CardValue::Seven,
      (x, CardValue::Seven) => x <= CardValue::Seven,
      (x, y) => x >= y,
   }
}

pub fn effective_top_card(pile: &[Card]) -> CardValue {
   pile
      .iter()
      .rev()
      .map(|x| x.value)
      .skip_while(|x| *x == CardValue::Four)
      .next()
      .unwrap_or(CardValue::Two)
}

#[derive(Debug, Serialize)]
pub struct PublicGameState<'a> {
   pub hands: Box<[u16]>,
   pub face_up_three: Box<[&'a [Card]]>,
   pub face_down_three: Box<[u8]>,
   pub top_card: Option<Card>,
   pub pile_size: u16,
   pub cleared_size: u16,
   pub cur_phase: Phase,
   pub active_player: u8,
   pub last_cards_played: &'a [Card],
   pub last_played_zone: Option<CardZone>,
}

mod test {
   #[cfg(test)]
   use super::*;

   #[cfg(test)]
   impl GameState {
      fn new_game_skip_setup(num_players: u8) -> GameState {
         let mut game = GameState::new(num_players);
         game.cur_phase = Phase::Play;
         game
      }

      fn play_card(&mut self, card_val: CardValue) -> Result<bool, &'static str> {
         let card = Card {
            value: card_val,
            suit: *SUITS.choose(&mut thread_rng()).unwrap(),
         };
         self.hands[self.active_player as usize] = vec![card];
         self.make_play(&[card])
      }
   }

   #[test]
   fn effective_top_card_works() {
      let mut game = GameState::new_game_skip_setup(4);
      assert_eq!(effective_top_card(&game.pile_cards), CardValue::Two);
      assert!(game.play_card(CardValue::Three).is_ok());
      assert_eq!(effective_top_card(&game.pile_cards), CardValue::Three);
      assert!(game.play_card(CardValue::Four).is_ok());
      assert_eq!(effective_top_card(&game.pile_cards), CardValue::Three);
   }

   #[test]
   fn normal_play() {
      let mut game = GameState::new_game_skip_setup(4);
      assert!(game.play_card(CardValue::Three).is_ok());
      assert_eq!(game.pile_cards.len(), 1);
      assert!(game.play_card(CardValue::Four).is_ok());
      assert_eq!(game.pile_cards.len(), 2);
      assert!(game.play_card(CardValue::Eight).is_ok());
      assert_eq!(game.pile_cards.len(), 3);
      assert_eq!(game.active_player, 3);
   }

   #[test]
   fn ten_clears_no_rotate() {
      let mut game = GameState::new_game_skip_setup(4);
      assert!(game.play_card(CardValue::Three).is_ok());
      assert_eq!(game.pile_cards.len(), 1);
      assert!(game.play_card(CardValue::Ten).is_ok());
      assert_eq!(game.pile_cards.len(), 0);
      assert_eq!(game.active_player, 1);
   }

   #[test]
   fn four_in_a_row_clears_no_rotate() {
      let mut game = GameState::new_game_skip_setup(4);
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert_eq!(game.pile_cards.len(), 0);
      assert_eq!(game.active_player, 3);

      // Same test but 4s only, a slightly trickier case
      assert!(game.play_card(CardValue::Ace).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert_eq!(game.pile_cards.len(), 0);
      assert_eq!(game.active_player, 3);

      // Same test but put 4s in the middle
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Four).is_ok());
      assert!(game.play_card(CardValue::Three).is_ok());
      assert_eq!(game.pile_cards.len(), 0);
      assert_eq!(game.active_player, 0);
   }

   #[test]
   fn sevens_invert_accepted_values() {
      let mut game = GameState::new_game_skip_setup(4);
      assert!(game.play_card(CardValue::Three).is_ok());
      assert!(game.play_card(CardValue::Seven).is_ok());
      assert!(game.play_card(CardValue::Eight).is_ok());
      assert_eq!(game.pile_cards.len(), 0);
      assert_eq!(game.active_player, 3);
   }

   #[test]
   fn less_than_4_players_less_than_4_suits() {
      for i in 2..=4 {
         let game = GameState::new(i);
         let mut seen_suits: Vec<CardSuit> = vec![];
         for hand in game.hands.iter() {
            for card in hand {
               seen_suits.push(card.suit);
            }
         }
         seen_suits.sort();
         seen_suits.dedup();
         assert_eq!(seen_suits.len() as u8, i);
      }
   }

   #[test]
   fn playing_ten_final_card_turn_rotates() {
      let mut game = GameState::new_game_skip_setup(4);
      assert_eq!(game.active_player, 0);
      game.face_up_three[0].clear();
      game.face_down_three[0].clear();
      game.play_card(CardValue::Ten).unwrap();
      assert_eq!(game.active_player, 1);
      assert!(game.out_players.contains(&0));
   }

   #[test]
   fn playing_ten_on_top_seven_rotates() {
      let mut game = GameState::new_game_skip_setup(3);
      assert_eq!(game.active_player, 0);
      game.play_card(CardValue::Seven).unwrap();
      assert_eq!(game.active_player, 1);
      game.play_card(CardValue::Ten).unwrap();
      assert_eq!(game.active_player, 2);
   }
}
