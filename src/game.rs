use rand::{self, Rng};

const HAND_SIZE: usize = 6;

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
enum CardSuit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

const SUITS: [CardSuit; 4] = [
    CardSuit::Clubs,
    CardSuit::Diamonds,
    CardSuit::Hearts,
    CardSuit::Spades,
];

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Card {
    value: CardValue,
    suit: CardSuit,
}

pub type CardTriplet = (Option<Card>, Option<Card>, Option<Card>);

#[derive(Copy, Clone, Debug, Serialize, PartialEq)]
pub enum GamePhase {
    Setup,
    Play,
}

#[derive(Clone, Debug)]
pub struct GameState {
    pub active_player: u8,
    num_players: u8,
    hands: Box<[Vec<Card>]>,
    face_up_three: Box<[CardTriplet]>,
    face_down_three: Box<[CardTriplet]>,
    cleared_cards: Vec<Card>,
    pile_cards: Vec<Card>,
    cur_phase: GamePhase,
}

impl GameState {
    pub fn new(num_players: u8) -> GameState {
        let mut deck: Vec<Card> = VALUES
            .iter()
            .cycle()
            .take(num_players as usize * VALUES.len())
            .zip(SUITS.iter().cycle())
            .map(|(value, suit)| Card {
                suit: *suit,
                value: *value,
            })
            .collect();
        rand::thread_rng().shuffle(&mut deck);
        let mut deck = deck.into_iter();
        let mut face_down_three = Vec::with_capacity(num_players as usize);
        let mut face_up_three = Vec::with_capacity(num_players as usize);
        let mut hands = Vec::with_capacity(num_players as usize);
        for _ in 0..num_players {
            face_down_three.push((
                Some(deck.next().unwrap()),
                Some(deck.next().unwrap()),
                Some(deck.next().unwrap()),
            ));
            face_up_three.push((
                Some(deck.next().unwrap()),
                Some(deck.next().unwrap()),
                Some(deck.next().unwrap()),
            ));
            hands.push(deck.by_ref().take(HAND_SIZE).collect());
        }
        GameState {
            active_player: 0,
            num_players: num_players,
            hands: hands.into_boxed_slice(),
            face_down_three: face_down_three.into_boxed_slice(),
            face_up_three: face_up_three.into_boxed_slice(),
            cleared_cards: Vec::new(),
            pile_cards: Vec::new(),
            cur_phase: GamePhase::Setup,
        }
    }

    pub fn public_state(&self) -> PublicGameState {
        let mut face_up_cards = vec![];
        for triplet in self.face_up_three.iter() {
            face_up_cards.push(Vec::new());
            if let Some(card) = triplet.0 {
                face_up_cards.last_mut().unwrap().push(card)
            }
            if let Some(card) = triplet.1 {
                face_up_cards.last_mut().unwrap().push(card)
            }
            if let Some(card) = triplet.2 {
                face_up_cards.last_mut().unwrap().push(card)
            }
        }
        let mut face_down_cards: Vec<u8> = vec![];
        for triplet in self.face_down_three.iter() {
            face_down_cards.push(0);
            if triplet.0.is_some() {
                *face_down_cards.last_mut().unwrap() += 1
            }
            if triplet.1.is_some() {
                *face_down_cards.last_mut().unwrap() += 1
            }
            if triplet.2.is_some() {
                *face_down_cards.last_mut().unwrap() += 1
            }
        }
        PublicGameState {
            hands: self
                .hands
                .iter()
                .map(|x| x.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            face_up_three: face_up_cards
                .into_iter()
                .map(|x| x.into_boxed_slice())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            face_down_three: face_down_cards.into_boxed_slice(),
            top_card: self.pile_cards.last().cloned(),
            pile_size: self.pile_cards.len(),
            cleared_size: self.cleared_cards.len(),
            cur_phase: self.cur_phase,
            active_player: self.active_player,
        }
    }

    pub fn choose_three_faceup(
        &mut self,
        card_one: Card,
        card_two: Card,
        card_three: Card,
    ) -> Result<(), &'static str> {
        // Validate phase
        if self.cur_phase != GamePhase::Setup {
            return Err("Can only choose three faceup cards during Setup phase");
        }

        // Combine hand + face up cards
        let mut all_cards = self.hands[self.active_player as usize].clone();
        let face_up_cards = self.face_up_three[self.active_player as usize];
        all_cards.push(face_up_cards.0.unwrap());
        all_cards.push(face_up_cards.1.unwrap());
        all_cards.push(face_up_cards.2.unwrap());

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
        self.face_up_three[self.active_player as usize] =
            (Some(card_one), Some(card_two), Some(card_three));
        self.hands[self.active_player as usize] = new_hand;
        self.rotate_play();

        if self.active_player == 0 {
            self.cur_phase = GamePhase::Play;
        }

        Ok(())
    }

    pub fn get_hand(&self, player_num: u8) -> &[Card] {
        &self.hands[player_num as usize]
    }

    fn rotate_play(&mut self) {
        self.active_player += 1;
        if self.active_player == self.num_players {
            self.active_player = 0;
        }
    }
}

#[derive(Serialize)]
pub struct PublicGameState {
    hands: Box<[usize]>,
    face_up_three: Box<[Box<[Card]>]>,
    face_down_three: Box<[u8]>,
    top_card: Option<Card>,
    pile_size: usize,
    cleared_size: usize,
    cur_phase: GamePhase,
    active_player: u8,
}

mod test {

    #[test]
    fn test_new_game() {
        use super::*;

        let new_game = GameState::new(4);
        let pub_state = new_game.public_state();
        let serialized = ::serde_json::to_string(&pub_state).unwrap();
        println!("{}", serialized);
    }
}
