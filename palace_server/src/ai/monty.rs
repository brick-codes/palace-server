// This AI plays random cards
use super::PalaceAi;
use crate::data::GameStartEvent;
use crate::game::{self, Card, CardZone, PublicGameState};
use crate::monte_game;
use noisy_float::prelude::*;
use rand::seq::SliceRandom;
use rand::{self, thread_rng};
use std::collections::HashMap;

const NUM_SIMULATIONS: usize = 1000;
const EXPLORATION_VAL: f64 = 0.7;

#[derive(Copy, Clone, Debug, PartialEq)]
enum MontyCard {
   Known(Card),
   Unknown,
}

impl MontyCard {
   fn unwrap_known(self) -> Card {
      match self {
         MontyCard::Known(c) => c,
         MontyCard::Unknown => unreachable!(),
      }
   }
}

#[derive(Debug)]
struct Node {
   last_move: Option<Box<[Card]>>,
   last_player: u8,
   parent: usize,
   simulations: u64,
   wins: u64,
   children: Vec<usize>,
}

fn ucb1(node: &Node, parent_simulations: u64) -> f64 {
   if node.simulations == 0 {
      return std::f64::INFINITY;
   }
   (node.wins as f64 / node.simulations as f64)
      + EXPLORATION_VAL * ((parent_simulations as f64).ln() / node.simulations as f64).sqrt()
}

impl From<Card> for MontyCard {
   fn from(c: Card) -> MontyCard {
      MontyCard::Known(c)
   }
}

#[derive(Debug)]
struct InformationSet {
   everyone_faceup_cards: Vec<Vec<Card>>,
   everyone_hands: Vec<Vec<MontyCard>>,
   everyone_facedown_cards: Vec<u8>,
   cur_pile: Vec<Card>,
   turn_number: u8,
}

impl InformationSet {
   fn new() -> InformationSet {
      InformationSet {
         everyone_faceup_cards: vec![],
         everyone_hands: vec![],
         everyone_facedown_cards: vec![],
         cur_pile: vec![],
         turn_number: 0,
      }
   }

   fn determine(&self, unseen_cards: &mut [Card]) -> monte_game::GameState {
      unseen_cards.shuffle(&mut thread_rng());
      let mut unseen_i = 0;

      let num_players = self.everyone_hands.len();

      // replace all unknown cards with unseen cards

      // first in hand
      let mut determined_hands = Vec::with_capacity(num_players);

      for known_hand in self.everyone_hands.iter() {
         determined_hands.push(Vec::with_capacity(known_hand.len()));
         for card in known_hand {
            let determined_card = match card {
               MontyCard::Known(c) => *c,
               MontyCard::Unknown => {
                  let c = unseen_cards[unseen_i];
                  unseen_i += 1;
                  c
               },
            };
            determined_hands.last_mut().unwrap().push(determined_card)
         }
         determined_hands.last_mut().unwrap().sort_unstable();
      }

      // then, face down cards
      let mut determined_fdt = Vec::with_capacity(num_players);

      for len in self.everyone_facedown_cards.iter().map(|x| *x) {
         determined_fdt.push(Vec::with_capacity(len as usize));
         for _ in 0..len {
            determined_fdt.last_mut().unwrap().push(unseen_cards[unseen_i]);
            unseen_i += 1;
         }
      }
      
      monte_game::GameState {
         active_player: self.turn_number,
         num_players: num_players as u8,
         hands: determined_hands.into_boxed_slice(),
         face_up_three: self.everyone_faceup_cards.clone().into_boxed_slice(),
         face_down_three: determined_fdt.into_boxed_slice(),
         pile_cards: self.cur_pile.clone(),
         cur_phase: game::Phase::Play,
         out_players: vec![],
      }
   }
}

pub struct MontyAi {
   information_set: InformationSet,
   last_player: u8,
   unseen_cards: HashMap<Card, u64>,
   last_phase: Option<game::Phase>,
}

pub fn new() -> MontyAi {
   MontyAi {
      information_set: InformationSet::new(),
      last_player: 0,
      unseen_cards: HashMap::new(),
      last_phase: None,
   }
}

/// relies on zone being sorted
fn all_moves_zone<'a>(zone: &'a [Card], v: &mut Vec<Box<[Card]>>) {
   let mut window_size: usize = 1;
   let mut found_window_at_size = true;
   while found_window_at_size {
      found_window_at_size = false;
      for window in zone.windows(window_size) {
         if window.iter().any(|x| x.value != window[0].value) {
            continue;
         }
         v.push(window.to_vec().into_boxed_slice());
         found_window_at_size = true;
      }
      window_size += 1;
   }
}

fn all_moves<'a>(g: &'a monte_game::GameState, v: &mut Vec<Box<[Card]>>) {
   if g.out_players.len() as u8 == g.num_players {
      // game over
      return;
   }
   let active_player_hand = &g.hands[g.active_player as usize];
   let active_player_fup3 = &g.face_up_three[g.active_player as usize];
   if !active_player_hand.is_empty() {
      all_moves_zone(active_player_hand, v);
   } else if !active_player_fup3.is_empty() {
      all_moves_zone(active_player_fup3, v);
   } else {
      v.push(vec![].into_boxed_slice());
   }
}

fn ismcts(root: &InformationSet, mut unseen_cards: Vec<Card>) -> Box<[Card]> {
   let mut tree: Vec<Node> = Vec::with_capacity(NUM_SIMULATIONS);
   tree.push(Node {
      last_move: None,
      parent: 0,
      wins: 0,
      simulations: 0,
      children: vec![],
      last_player: 0,
   });

   let mut moves = vec![];
   for _ in 0..NUM_SIMULATIONS {
      // determine state
      let mut g = root.determine(&mut unseen_cards);
      // select
      let mut cur_node = 0;
      while tree[cur_node].simulations > 0 {
         moves.clear();
         all_moves(&g, &mut moves);

         for a_move in moves.iter() {
            if !tree[cur_node].children.iter().any(|x| tree[*x].last_move.as_ref() == Some(a_move)) {
               let newl = tree.len();
               tree[cur_node].children.push(newl);
               tree.push(Node {
                  last_move: Some(a_move.clone()),
                  parent: cur_node,
                  wins: 0,
                  simulations: 0,
                  children: vec![],
                  last_player: g.active_player,
               });
            }
         }

         // traverse until a leaf
         cur_node = *tree[cur_node]
            .children
            .iter()
            .filter(|x| moves.contains(tree[**x].last_move.as_ref().unwrap()))
            .max_by_key(|x| r64(ucb1(&tree[**x], tree[cur_node].simulations)))
            .unwrap();
         if tree[cur_node].children.is_empty() {
            // terminal node
            break;
         }
         g.make_play(tree[cur_node].last_move.as_ref().unwrap()).unwrap();
      }
      // we reached a leaf
      // simulate
      let mut winner = tree[cur_node].last_player;
      while (g.out_players.len() as u8) < g.num_players {
         // make a random move
         let rand_move = {
            moves.clear();
            all_moves(&g, &mut moves);
            moves.choose(&mut thread_rng()).unwrap()
         };
         winner = g.active_player;
         g.make_play(&rand_move).unwrap();
      }
      // backprop
      loop {
         if tree[cur_node].last_player == winner {
            tree[cur_node].wins += 1;
         }
         tree[cur_node].simulations += 1;
         if cur_node == 0 {
            break;
         }
         cur_node = tree[cur_node].parent;
      }
   }

   // simulations done, choose best move
   let best_child = tree[0].children.iter().max_by_key(|x| tree[**x].simulations).unwrap();
   //println!("---");
   for child in tree[0].children.iter() {
      //println!("CHILD: {} sims, {} wins", tree[*child].simulations, tree[*child].wins);
   }
   tree[*best_child].last_move.clone().unwrap()
}

impl PalaceAi for MontyAi {
   fn strategy_name(&self) -> &'static str {
      "Monty"
   }

   fn choose_three_faceup(&mut self) -> (Card, Card, Card) {
      let my_hand = &self.information_set.everyone_hands[self.information_set.turn_number as usize];
      (
         my_hand[my_hand.len() - 1].unwrap_known(),
         my_hand[my_hand.len() - 2].unwrap_known(),
         my_hand[my_hand.len() - 3].unwrap_known(),
      )
   }

   fn take_turn(&mut self) -> Box<[Card]> {
      let mut unseen_cards: Vec<Card> = Vec::new();
      for (unseen_card, quantity) in self.unseen_cards.iter() {
         for _ in 0..*quantity {
            unseen_cards.push(*unseen_card);
         }
      }

      ismcts(&self.information_set, unseen_cards)
   }

   fn on_game_state_update(&mut self, new_state: &PublicGameState) {
      self.information_set.everyone_facedown_cards.clear();
      self
         .information_set
         .everyone_facedown_cards
         .extend_from_slice(&*new_state.face_down_three);

      if self.last_phase.is_none() {
         for i in 0..new_state.face_up_three.len() {
            self.information_set.everyone_hands[i].extend(new_state.face_up_three[i].iter().map(|x| {
               let y: MontyCard = (*x).into();
               y
            }));
            for card in new_state.face_up_three[i] {
               let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
               debug_assert!(*num_unseen > 0);
               *num_unseen -= 1;
            }
         }
         self.last_phase = Some(game::Phase::Setup)
      } else if self.last_phase == Some(game::Phase::Setup) && new_state.cur_phase == game::Phase::Play {
         for i in 0..new_state.face_up_three.len() {
            self.information_set.everyone_faceup_cards[i].extend_from_slice(new_state.face_up_three[i]);
            for card in new_state.face_up_three[i].iter() {
               let remove_result = self.information_set.everyone_hands[i].remove_item(&(*card).into());
               if remove_result.is_none() {
                  self.information_set.everyone_hands[i]
                     .remove_item(&MontyCard::Unknown)
                     .unwrap();
                  let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
                  debug_assert!(*num_unseen > 0);
                  *num_unseen -= 1;
               }
            }
         }
         self.last_phase = Some(game::Phase::Play);
      }

      // update pile based on cards played
      self
         .information_set
         .cur_pile
         .extend_from_slice(new_state.last_cards_played);

      let last_player_hand = &mut self.information_set.everyone_hands[self.last_player as usize];

      // update hand based on cards played
      match new_state.last_played_zone {
         Some(CardZone::Hand) => {
            for card in new_state.last_cards_played {
               let remove_result = last_player_hand.remove_item(&(*card).into());
               if remove_result.is_none() {
                  last_player_hand.remove_item(&MontyCard::Unknown).unwrap();
                  let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
                  debug_assert!(*num_unseen > 0);
                  *num_unseen -= 1;
               }
            }
         }
         Some(CardZone::FaceUpThree) => {
            let last_player_faceup = &mut self.information_set.everyone_faceup_cards[self.last_player as usize];
            for card in new_state.last_cards_played {
               last_player_faceup.remove_item(card).unwrap();
            }
         }
         Some(CardZone::FaceDownThree) => {
            debug_assert_eq!(new_state.last_cards_played.len(), 1);
            for card in new_state.last_cards_played {
               let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
               debug_assert!(*num_unseen > 0);
               *num_unseen -= 1;
            }
         }
         None => (),
      }

      // If pile got picked up, update hand
      if new_state.pile_size == 0 {
         // update hand of last player to include all of the pile cards
         // IF they did not clear it
         if self.last_player != new_state.active_player {
            last_player_hand.extend(self.information_set.cur_pile.iter().map(|x| {
               let y: MontyCard = (*x).into();
               y
            }));
         }
         self.information_set.cur_pile.clear();
      }

      self.last_player = new_state.active_player;
   }

   fn on_game_start(&mut self, game_start_event: GameStartEvent) {
      let num_players = game_start_event.players.len();

      self.unseen_cards.reserve(num_players * 13);
      for card in game::new_deck(num_players) {
         *self.unseen_cards.entry(card).or_insert(0) += 1;
      }
      self.information_set.everyone_hands.reserve_exact(num_players);
      self.information_set.everyone_faceup_cards.reserve_exact(num_players);
      for _ in 0..num_players {
         self
            .information_set
            .everyone_hands
            .push(vec![MontyCard::Unknown; crate::game::HAND_SIZE]);
         self.information_set.everyone_faceup_cards.push(Vec::with_capacity(3));
      }
      self.information_set.turn_number = game_start_event.turn_number;
      for card in game_start_event.hand {
         *self.unseen_cards.get_mut(card).unwrap() -= 1;
      }
      let my_hand = &mut self.information_set.everyone_hands[self.information_set.turn_number as usize];
      my_hand.clear();
      my_hand.extend(game_start_event.hand.iter().map(|x| {
         let y: MontyCard = (*x).into();
         y
      }));
   }

   fn on_hand_update(&mut self, _new_hand: &[Card]) {
      // we just manage our own hand naturally
   }
}
