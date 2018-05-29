use rand::{self, Rng};
use std::ops::{Deref, DerefMut};

#[derive(Copy, Clone, Debug)]
enum CardSuit {
    Clubs,
    Diamonds,
    Hearts,
    Spades
}

const SUITS: [CardSuit; 4] = [CardSuit::Clubs, CardSuit::Diamonds, CardSuit::Hearts, CardSuit::Spades];

#[derive(Copy, Clone, Debug)]
enum CardValue {
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
    Ace
}

const VALUES: [CardValue; 13] = [CardValue::Two, CardValue::Three, CardValue::Four, CardValue::Five, CardValue::Six, CardValue::Seven, CardValue::Eight, CardValue::Nine, CardValue::Ten, CardValue::Jack, CardValue::Queen, CardValue::King, CardValue::Ace];

struct Card {
    value: CardValue,
    suit: CardSuit
}

pub enum Game {
    Setup(SetupGameState),
    Play(PlayGameState)
}

impl Game {
    fn new(num_players: u8) -> Game {
        let mut deck: Vec<Card> = VALUES.iter().cycle().take(num_players as usize * VALUES.len()).zip(SUITS.iter().cycle()).map(|(value, suit)| Card { suit: *suit, value: *value }).collect();
        rand::thread_rng().shuffle(&mut deck);
        let mut deck = deck.into_iter();
        let mut face_down_three = Vec::with_capacity(num_players as usize);
        let mut face_up_three = Vec::with_capacity(num_players as usize);
        let mut hands = Vec::with_capacity(num_players as usize);
        for _ in 0..num_players {
            face_down_three.push((Some(deck.next().unwrap()), Some(deck.next().unwrap()), Some(deck.next().unwrap())));
            face_up_three.push((Some(deck.next().unwrap()), Some(deck.next().unwrap()), Some(deck.next().unwrap())));
            hands.push(deck.by_ref().take(6).collect());
        }
        let game_state = GameState {
            active_player: 0,
            hands: hands.into_boxed_slice(),
            face_down_three: face_down_three.into_boxed_slice(),
            face_up_three: face_up_three.into_boxed_slice(),
            cleared_cards: Vec::new(),
        };
        Game::Setup(SetupGameState {
            inner: game_state
        })
    }
}

struct SetupGameState {
    inner: GameState
}

struct PlayGameState {
    inner: GameState
}

impl Deref for SetupGameState {
    type Target = GameState;

    fn deref(&self) -> &GameState {
        &self.inner
    }
}

impl Deref for PlayGameState {
    type Target = GameState;

    fn deref(&self) -> &GameState {
        &self.inner
    }
}

impl DerefMut for SetupGameState {
    fn deref_mut(&mut self) -> &mut GameState {
        &mut self.inner
    }
}

impl DerefMut for PlayGameState {
    fn deref_mut(&mut self) -> &mut GameState {
        &mut self.inner
    }
}

impl SetupGameState {
    fn set_three_face_up(&mut self, card_to_keep1: Card, card_to_keep2: Card, card_to_keep3: Card) {
        let new_face_up_cards = (Some(card_to_keep1), Some(card_to_keep2), Some(card_to_keep3));
    }
}

type CardTriplet = (Option<Card>, Option<Card>, Option<Card>);

struct GameState {
    active_player: u8,
    hands: Box<[Vec<Card>]>,
    face_up_three: Box<[CardTriplet]>,
    face_down_three: Box<[CardTriplet]>,
    cleared_cards: Vec<Card>
}

mod test {
    use super::*;

    #[test]
    fn test_new_game() {
        let new_game = Game::new(4);
    }
}
