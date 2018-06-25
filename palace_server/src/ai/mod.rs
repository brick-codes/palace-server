use data::GameStartEvent;
use game::{Card, PublicGameState};
use rand::{self, Rng};

pub mod random;

lazy_static! {
   static ref NAMES: Vec<&'static str> = { include_str!("names.txt").lines().collect() };
   static ref ADJECTIVES: Vec<&'static str> = { include_str!("adjectives.txt").lines().collect() };
   static ref NOUNS: Vec<&'static str> = { include_str!("nouns.txt").lines().collect() };
}

static LETTERS: [char; 26] = [
   'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W',
   'X', 'Y', 'Z',
];

pub(crate) trait PalaceAi {
   fn strategy_name(&self) -> &'static str;

   fn choose_three_faceup(&mut self) -> (Card, Card, Card);

   fn take_turn(&mut self) -> Box<[Card]>;

   fn on_game_state_update(&mut self, _new_state: &PublicGameState) {}

   fn on_game_start(&mut self, _game_start_event: GameStartEvent) {}

   fn on_hand_update(&mut self, _new_hand: &[Card]) {}
}

pub(crate) fn get_bot_name() -> String {
   format!("BOT {}", rand::thread_rng().choose(&NAMES).unwrap())
}

pub(crate) fn get_bot_name_clandestine() -> String {
   let base = match rand::thread_rng().gen_range(0, 5) {
      0 => rand::thread_rng().choose(&NAMES).unwrap().to_string(),
      1 => rand::thread_rng().choose(&NAMES).unwrap().to_ascii_lowercase(),
      2 => format!(
         "{}{}",
         rand::thread_rng().choose(&NAMES).unwrap(),
         rand::thread_rng().choose(&LETTERS).unwrap()
      ),
      3 => rand::thread_rng().choose(&NOUNS).unwrap().to_string(),
      4 => format!(
         "{}{}",
         rand::thread_rng().choose(&ADJECTIVES).unwrap(),
         rand::thread_rng().choose(&NOUNS).unwrap()
      ),
      _ => unreachable!(),
   };
   let suffix = match rand::thread_rng().gen_range(0, 4) {
      0 => "".to_string(),
      1 => {
         let mut suffix = rand::thread_rng().gen_range(0, 10).to_string();
         while rand::random() {
            suffix = format!("{}{}", suffix, rand::thread_rng().gen_range(0, 10));
         }
         suffix
      }
      2 => rand::thread_rng().gen_range(80, 100).to_string(),
      3 => rand::thread_rng().gen_range(1980, 2001).to_string(),
      _ => unreachable!(),
   };
   format!("{}{}", base, suffix)
}