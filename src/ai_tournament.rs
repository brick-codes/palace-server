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
   Monty(f64, usize),
   LowAndSteady,
}

impl Ai {
   fn instantiate(self) -> Box<dyn PalaceAi + Send + Sync> {
      match self {
         Ai::Random => Box::new(ai::random::new()),
         Ai::Monty(c, sims) => Box::new(ai::monty::with_parameters(c, sims)),
         Ai::LowAndSteady => Box::new(ai::low_and_steady::new()),
      }
   }
}

impl Display for Ai {
   fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
      match self {
         Ai::Random => write!(f, "Random"),
         Ai::Monty(c, sims) => write!(f, "Monty (c={}, {} sims)", c, sims),
         Ai::LowAndSteady => write!(f, "Low and Steady"),
      }
   }
}

const AI_ARRAY: [Ai; 3] = [Ai::Random, Ai::Monty(0.7, 1000), Ai::LowAndSteady];

fn ai_play(game: &mut GameState, ai_core: &mut (dyn PalaceAi + Send + Sync)) -> bool {
   let cards_to_play = ai::get_turn(game, ai_core);
   game.take_turn(&cards_to_play).unwrap()
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

struct DuelResult {
   first_wins: usize,
   second_wins: usize,
   draws: usize,
   total_turns: usize,
}

fn ai_duel(first_ai: Ai, second_ai: Ai) -> DuelResult {
   let i_wins: AtomicUsize = AtomicUsize::new(0);
   let j_wins: AtomicUsize = AtomicUsize::new(0);
   let draws: AtomicUsize = AtomicUsize::new(0);
   let total_turns: AtomicUsize = AtomicUsize::new(0);
   println!("{} vs. {}", first_ai, second_ai);
   (0..NUM_GAMES / 2).into_par_iter().for_each(|_| {
      let result = run_ai_game(first_ai, second_ai);
      match result.winner {
         Winner::Player2 => j_wins.fetch_add(1, Ordering::Relaxed),
         Winner::Player1 => i_wins.fetch_add(1, Ordering::Relaxed),
         Winner::TimedOut => draws.fetch_add(1, Ordering::Relaxed),
      };
      total_turns.fetch_add(result.num_turns, Ordering::Relaxed);
   });
   (0..NUM_GAMES / 2).into_par_iter().for_each(|_| {
      let result = run_ai_game(second_ai, first_ai);
      match result.winner {
         Winner::Player2 => i_wins.fetch_add(1, Ordering::Relaxed),
         Winner::Player1 => j_wins.fetch_add(1, Ordering::Relaxed),
         Winner::TimedOut => draws.fetch_add(1, Ordering::Relaxed),
      };
      total_turns.fetch_add(result.num_turns, Ordering::Relaxed);
   });
   DuelResult {
      first_wins: i_wins.into_inner(),
      second_wins: j_wins.into_inner(),
      draws: draws.into_inner(),
      total_turns: total_turns.into_inner(),
   }
}

pub fn monty_report() {
   let exploration_vals = [0.7, std::f64::consts::SQRT_2];
   let num_simulations = [25, 50, 100, 250, 500, 1000, 2000];
   let opponents = [Ai::LowAndSteady, Ai::Random];
   let mut outputs = vec![];

   use std::fs::File;
   use std::io::{BufWriter, Write};

   for opponent in opponents.iter() {
      let name = format!("vs_{}.csv", opponent);
      outputs.push(BufWriter::new(File::create(&name).unwrap()))
   }

   for output in outputs.iter_mut() {
      writeln!(
         output,
         "exploration_constant,number_of_simulations,wins,losses,draws,score,total_turns"
      )
      .unwrap();
   }

   for c_val in exploration_vals.iter() {
      for num_sims in num_simulations.iter() {
         let monty = Ai::Monty(*c_val, *num_sims);
         for (opponent, output) in opponents.iter().zip(outputs.iter_mut()) {
            let vs_result = ai_duel(monty, *opponent);
            writeln!(
               output,
               "{},{},{},{},{},{},{}",
               c_val,
               num_sims,
               vs_result.first_wins,
               vs_result.second_wins,
               vs_result.draws,
               vs_result.first_wins as f64 + vs_result.draws as f64 / 2.0,
               vs_result.total_turns
            )
            .unwrap();
         }
      }
   }
}

pub fn go() {
   for (i, ai_1) in AI_ARRAY.iter().enumerate() {
      let ai_1 = *ai_1;
      for ai_2 in AI_ARRAY[i + 1..].iter() {
         let ai_2 = *ai_2;
         let res = ai_duel(ai_1, ai_2);
         println!(
            "{}: {} wins ({:.2}%) // {}: {} ({:.2}%) || {} draws || avg. game length: {:.2} turns",
            ai_1,
            res.first_wins,
            res.first_wins as f64 / NUM_GAMES as f64,
            ai_2,
            res.second_wins,
            res.second_wins as f64 / NUM_GAMES as f64,
            res.draws as f64,
            res.total_turns as f64 / NUM_GAMES as f64
         )
      }
   }
}
