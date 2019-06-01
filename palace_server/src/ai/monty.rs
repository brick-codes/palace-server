// This AI plays random cards
use super::PalaceAi;
use crate::data::GameStartEvent;
use crate::game::{self, Card, CardZone, PublicGameState};
use crate::monte_game;
use rand::seq::SliceRandom;
use rand::{self, thread_rng};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq)]
enum MontyCard {
   Known(Card),
   Unknown,
}

#[derive(Debug)]
struct Node {
   state: monte_game::GameState,
   parent: usize,
   simulations: u64,
   wins: u64,
   children: Vec<usize>,
   owner: u8,
}

fn uct(node: &Node, parent_simulations: u64) -> f64 {
   let c: f64 = 2.0f64.sqrt();
   (node.wins as f64 / (node.simulations + 1) as f64) + c * ((parent_simulations as f64).ln() / (node.simulations + 1) as f64).sqrt()
}

impl From<Card> for MontyCard {
   fn from(c: Card) -> MontyCard {
      MontyCard::Known(c)
   }
}

pub struct MontyAi {
   everyone_faceup_cards: Vec<Vec<Card>>,
   everyone_hands: Vec<Vec<MontyCard>>,
   everyone_facedown_cards: Vec<u8>,
   cur_pile: Vec<Card>,
   turn_number: u8,
   last_player: u8,
   unseen_cards: HashMap<Card, u64>,
   last_phase: Option<game::Phase>,
   official_hand: Vec<MontyCard>,
}

pub fn new() -> MontyAi {
   MontyAi {
      everyone_faceup_cards: vec![],
      everyone_hands: vec![],
      everyone_facedown_cards: vec![],
      turn_number: 0,
      cur_pile: vec![],
      last_player: 0,
      unseen_cards: HashMap::new(),
      last_phase: None,
      official_hand: vec![],
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

fn mcts(g: monte_game::GameState) -> Box<[Card]> {
   let mut tree: Vec<Node> = vec![Node { state: g, parent: 0, wins: 0, simulations: 0, children: vec![], owner: 0 }];

   let mut moves = vec![];
   for _ in 0..1000 {
      // select
      let mut cur_node = 0;
      while tree[cur_node].simulations > 0 {
         if tree[cur_node].children.is_empty() {
            // expand children (by movegen)
            moves.clear();
            all_moves(&tree[cur_node].state, &mut moves);
            for a_move in moves.iter() {
               let mut new_state = tree[cur_node].state.clone();
               let game_over = new_state.make_play(a_move).unwrap();
               if !game_over {
                  let newl = tree.len();
                  tree[cur_node].children.push(newl);
                  tree.push(Node {
                     state: new_state,
                     parent: cur_node,
                     wins: 0,
                     simulations: 0,
                     children: vec![],
                     owner: tree[cur_node].state.active_player,
                  });
               }
            }
         }
         if tree[cur_node].children.is_empty() {
            // expansion "failed" because all moves go to terminal state
            // that means this node is equivalent to a win
            // not sure what to do?
            break;
         }
         // traverse until a leaf
         let mut next_node: usize = tree[cur_node].children[0];
         let mut best_score = uct(&tree[next_node], tree[cur_node].simulations);
         for child in tree[cur_node].children.iter().skip(1) {
            let score = uct(&tree[*child], tree[cur_node].simulations);
            if score > best_score {
               best_score = score;
               next_node = *child;
            }
         }
         cur_node = next_node;
         // TODO: nosiy float
         // let cur_node = cur_node.children.iter().max_by_key(|x| r64(uct(x, cur_node)));
      }
      // we reached a leaf
      // simulate
      let mut simulate_game = tree[cur_node].state.clone();
      let mut game_over = false;
      let mut winner = 0;
      while !game_over {
         // make a random move
         let rand_move = {
            moves.clear();
            all_moves(&simulate_game, &mut moves);
            moves.choose(&mut thread_rng()).unwrap()
         };
         winner = simulate_game.active_player;
         game_over = simulate_game.make_play(&rand_move).unwrap();
      }
      // backprop
      loop {
         if tree[cur_node].owner == winner {
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
   let best_move = tree[0].children.iter().enumerate().max_by_key(|(_i, x)| tree[**x].simulations).map(|x| x.0).unwrap_or(0);
   moves.clear();
   all_moves(&mut tree[0].state, &mut moves);
   //println!("{} sims, {} wins", tree[0].simulations, tree[0].wins);
   for (i, child) in tree[0].children.iter().enumerate() {
      //println!("child: {} sims, {} wins (move: {:?})", tree[*child].simulations, tree[*child].wins, moves[i]);
   }
   moves[best_move].to_vec().into_boxed_slice()
}

impl PalaceAi for MontyAi {
   fn strategy_name(&self) -> &'static str {
      "Monty"
   }

   fn choose_three_faceup(&mut self) -> (Card, Card, Card) {
      let my_faceup = &self.everyone_faceup_cards[self.turn_number as usize];
      (my_faceup[0], my_faceup[1], my_faceup[2])
   }

   fn take_turn(&mut self) -> Box<[Card]> {
      let num_players = self.everyone_hands.len();
      let hands = vec![vec![]; num_players].into_boxed_slice();
      let face_up_three = vec![vec![]; num_players].into_boxed_slice();
      let face_down_three = vec![vec![]; num_players].into_boxed_slice();
      let mut g = monte_game::GameState {
         active_player: self.turn_number,
         num_players: num_players as u8,
         hands,
         face_up_three,
         face_down_three,
         pile_cards: Vec::with_capacity(num_players * 13),
         cur_phase: game::Phase::Play,
         out_players: vec![],
      };

      let mut unseen_cards: Vec<Card> = Vec::new();
      for (unseen_card, quantity) in self.unseen_cards.iter() {
         for _ in 0..*quantity {
            unseen_cards.push(*unseen_card);
         }
      }

      let mut best_moves = HashMap::with_capacity(10);
      for _ in 0..10 {
         g.out_players.clear();
         g.pile_cards = self.cur_pile.clone();
         g.face_up_three = self.everyone_faceup_cards.clone().into_boxed_slice();

         let mut determination_cards: Vec<Card> = unseen_cards.clone();
         //println!("{}", determination_cards.len());
         determination_cards.shuffle(&mut thread_rng());

         // replace all unknown cards with unseen cards
         // first in hand
         for (known_hand, determined_hand) in self.everyone_hands.iter().zip(g.hands.iter_mut()) {
            determined_hand.clear();
            for card in known_hand {
               let determined_card = match card {
                  MontyCard::Known(c) => {
                     *c
                  }
                  MontyCard::Unknown => {
                     determination_cards.pop().unwrap()
                  }
               };
               determined_hand.push(determined_card)
            }
            determined_hand.sort_unstable();
         }
         // then, face down cards
         for (i, determined_fdt) in g.face_down_three.iter_mut().enumerate() {
            determined_fdt.clear();
            for _ in 0..self.everyone_facedown_cards[i] {
               determined_fdt.push(determination_cards.pop().unwrap());
            }
         }

         // determination is now determined.
         *best_moves.entry(mcts(g.clone())).or_insert(0u64) += 1;
      }
      //println!("{:?}", best_moves[0]);
      return best_moves.iter().max_by_key(|(_am, freq)| *freq).unwrap().0.clone();
   }

   fn on_game_state_update(&mut self, new_state: &PublicGameState) {
      self.everyone_facedown_cards.clear();
      self
         .everyone_facedown_cards
         .extend_from_slice(&*new_state.face_down_three);

      if self.last_phase.is_none() {
         for i in 0..new_state.face_up_three.len() {
            self.everyone_faceup_cards[i].extend_from_slice(new_state.face_up_three[i]);
            for card in new_state.face_up_three[i] {
               let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
               debug_assert!(*num_unseen > 0);
               *num_unseen -= 1;
            }
         }
         self.last_phase = Some(game::Phase::Setup)
      } else if self.last_phase == Some(game::Phase::Setup) && new_state.cur_phase == game::Phase::Play {
         for i in 0..new_state.face_up_three.len() {
            // TODO: diff existing faceup and new faceup, update unseen and hands accordingly
            self.everyone_faceup_cards[i].clear();
            self.everyone_faceup_cards[i].extend_from_slice(new_state.face_up_three[i]);
         }
         self.last_phase = Some(game::Phase::Play);
      }

      // update pile based on cards played
      self.cur_pile.extend_from_slice(new_state.last_cards_played);

      let last_player_hand = &mut self.everyone_hands[self.last_player as usize];

      if self.last_player == self.turn_number {
         //print!("me: ");
      } else {
         //print!("other: ");
      }

      // update hand based on cards played
      match new_state.last_played_zone {
         Some(CardZone::Hand) => {
            //println!("a card was played from hand");
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
            //println!("a card was played from fup3");
            let last_player_faceup = &mut self.everyone_faceup_cards[self.last_player as usize];
            for card in new_state.last_cards_played {
               last_player_faceup.remove_item(card).unwrap();
            }
         }
         Some(CardZone::FaceDownThree) => {
            //println!("a card was played from fd3");
            debug_assert_eq!(new_state.last_cards_played.len(), 1);
            for card in new_state.last_cards_played {
               let num_unseen = self.unseen_cards.get_mut(&card).unwrap();
               debug_assert!(*num_unseen > 0);
               *num_unseen -= 1;
            }
         }
         None => {
            //println!("we dont care");
         }
      }

      // If pile got picked up, update hand
      if new_state.pile_size == 0 {
         // update hand of last player to include all of the pile cards
         // IF they did not clear it
         if self.last_player != new_state.active_player {
            last_player_hand.extend(self.cur_pile.iter().map(|x| {
               let y: MontyCard = (*x).into();
               y
            }));
            //println!("picked up");
         } else {
            //println!("cleared");
         }
         self.cur_pile.clear();
      } else {
         //println!("did not pick up or clear");
      }

      self.last_player = new_state.active_player;
      //println!("lcp: {:?}", new_state.last_cards_played);
      debug_assert_eq!(&self.official_hand, &self.everyone_hands[self.turn_number as usize]);
   }

   fn on_game_start(&mut self, game_start_event: GameStartEvent) {
      let num_players = game_start_event.players.len();

      self.unseen_cards.reserve(num_players * 13);
      for card in game::new_deck(num_players) {
         *self.unseen_cards.entry(card).or_insert(0) += 1;
      }
      self.everyone_hands.reserve_exact(num_players);
      self.everyone_faceup_cards.reserve_exact(num_players);
      for _ in 0..num_players {
         self
            .everyone_hands
            .push(vec![MontyCard::Unknown; crate::game::HAND_SIZE]);
         self.everyone_faceup_cards.push(Vec::with_capacity(3));
      }
      self.turn_number = game_start_event.turn_number;
      for card in game_start_event.hand {
         *self.unseen_cards.get_mut(card).unwrap() -= 1;
      }
      let my_hand = &mut self.everyone_hands[self.turn_number as usize];
      my_hand.clear();
      my_hand.extend(game_start_event.hand.iter().map(|x| {
         let y: MontyCard = (*x).into();
         y
      }));
      self.official_hand.extend(game_start_event.hand.iter().map(|x| {
            let y: MontyCard = (*x).into();
            y
         }));
   }

   fn on_hand_update(&mut self, new_hand: &[Card]) {
      self.official_hand.clear();
      self.official_hand.extend(new_hand.iter().map(|x| {
            let y: MontyCard = (*x).into();
            y
         }));
      // we just manage our own hand naturally
   }
}
