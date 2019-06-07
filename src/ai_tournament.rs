use palace_server::ai::{self, PalaceAi};
use palace_server::game::GameState;
use std::collections::HashMap;
use std::fmt::{self, Display};

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const NUM_GAMES: usize = 1000;

#[derive(Copy, Clone, Debug)]
enum Ai {
   Random,
   Monty,
   LowAndSteady,
}

impl Ai {
   fn instantiate(self) -> Box<dyn PalaceAi + Send + Sync> {
      match self {
         Ai::Random => Box::new(ai::random::new()),
         Ai::Monty => Box::new(ai::monty::new()),
         Ai::LowAndSteady => Box::new(ai::low_and_steady::new()),
      }
   }
}

impl Display for Ai {
   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      let s = match self {
         Ai::Random => "Random",
         Ai::Monty => "Monty",
         Ai::LowAndSteady => "Low and Steady",
      };
      write!(f, "{}", s)
   }
}

const AI_ARRAY: [Ai; 3] = [Ai::Random, Ai::Monty, Ai::LowAndSteady];

fn ai_play(game: &mut GameState, ai_core: &mut (dyn PalaceAi + Send + Sync)) -> bool {
   match game.cur_phase {
      palace_server::game::Phase::Setup => {
         let faceup_three = ai_core.choose_three_faceup();
         game
            .choose_three_faceup(faceup_three.0, faceup_three.1, faceup_three.2)
            .unwrap();
         false
      }
      palace_server::game::Phase::Play => {
         let cards_to_play = ai::get_play(game, ai_core);
         game.make_play(&cards_to_play).unwrap()
      }
   }
}

#[derive(PartialEq)]
enum Winner {
   Player1,
   Player2,
   TimedOut,
}

struct GameResult {
   winner: Winner,
   num_turns: usize,
}

fn run_ai_game(first_player: Ai, second_player: Ai) -> GameResult {
   let mut game = palace_server::game::GameState::new(2);
   let mut num_turns = 0;
   assert_eq!(game.active_player, 0);
   let mut first_player = first_player.instantiate();
   let mut second_player = second_player.instantiate();
   let mut players = HashMap::with_capacity(2);
   players.insert(0, first_player.strategy_name().into());
   players.insert(1, second_player.strategy_name().into());
   let gse_1 = palace_server::data::GameStartEvent {
      hand: game.get_hand(0),
      turn_number: 0,
      players: &players,
   };
   let gse_2 = palace_server::data::GameStartEvent {
      hand: game.get_hand(1),
      turn_number: 1,
      players: &players,
   };
   first_player.on_game_start(gse_1);
   second_player.on_game_start(gse_2);
   let pgs = game.public_state();
   first_player.on_game_state_update(&pgs);
   second_player.on_game_state_update(&pgs);
   loop {
      num_turns += 1;
      if game.active_player == 0 {
         if ai_play(&mut game, &mut *first_player) {
            return GameResult {
               winner: Winner::Player1,
               num_turns,
            };
         }
         let pgs = game.public_state();
         first_player.on_hand_update(game.get_hand(0));
         first_player.on_game_state_update(&pgs);
         second_player.on_game_state_update(&pgs)
      } else {
         if ai_play(&mut game, &mut *second_player) {
            return GameResult {
               winner: Winner::Player2,
               num_turns,
            };
         }
         let pgs = game.public_state();
         second_player.on_hand_update(game.get_hand(1));
         first_player.on_game_state_update(&pgs);
         second_player.on_game_state_update(&pgs);
      }
      if num_turns >= 1000 {
         return GameResult {
            winner: Winner::TimedOut,
            num_turns,
         };
      }
   }
}

pub fn go() {
   for i in 0..AI_ARRAY.len() {
      for j in i + 1..AI_ARRAY.len() {
         let i_wins: AtomicUsize = AtomicUsize::new(0);
         let j_wins: AtomicUsize = AtomicUsize::new(0);
         let draws: AtomicUsize = AtomicUsize::new(0);
         let total_turns: AtomicUsize = AtomicUsize::new(0);
         println!("{} vs. {}", AI_ARRAY[i], AI_ARRAY[j]);
         (0..NUM_GAMES / 2).into_par_iter().for_each(|_| {
            let result = run_ai_game(AI_ARRAY[i], AI_ARRAY[j]);
            match result.winner {
               Winner::Player2 => j_wins.fetch_add(1, Ordering::Relaxed),
               Winner::Player1 => i_wins.fetch_add(1, Ordering::Relaxed),
               Winner::TimedOut => draws.fetch_add(1, Ordering::Relaxed),
            };
            total_turns.fetch_add(result.num_turns, Ordering::Relaxed);
         });
         (0..NUM_GAMES / 2).into_par_iter().for_each(|_| {
            let result = run_ai_game(AI_ARRAY[j], AI_ARRAY[i]);
            match result.winner {
               Winner::Player2 => i_wins.fetch_add(1, Ordering::Relaxed),
               Winner::Player1 => j_wins.fetch_add(1, Ordering::Relaxed),
               Winner::TimedOut => draws.fetch_add(1, Ordering::Relaxed),
            };
            total_turns.fetch_add(result.num_turns, Ordering::Relaxed);
         });
         let i_wins = i_wins.into_inner();
         let j_wins = j_wins.into_inner();
         let draws = draws.into_inner();
         let total_turns = total_turns.into_inner();
         println!(
            "{}: {} wins ({:.2}%) // {}: {} ({:.2}%) || {} draws || avg. game length: {:.2} turns",
            AI_ARRAY[i],
            i_wins,
            i_wins as f64 / NUM_GAMES as f64,
            AI_ARRAY[j],
            j_wins,
            j_wins as f64 / NUM_GAMES as f64,
            draws as f64,
            total_turns as f64 / NUM_GAMES as f64
         )
      }
   }
}
