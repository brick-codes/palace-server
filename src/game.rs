use rand::{self, Rng};
use std::iter::FromIterator;

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

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd)]
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
    face_up_three: Box<[Vec<Card>]>,
    face_down_three: Box<[Vec<Card>]>,
    cleared_cards: Vec<Card>,
    pile_cards: Vec<Card>,
    cur_phase: GamePhase,
    last_cards_played: Vec<Card>,
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
            face_up_three.push(deck.by_ref().take(3).collect());
            face_down_three.push(deck.by_ref().take(3).collect());
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
            last_cards_played: Vec::new(),
        }
    }

    pub fn public_state(&self) -> PublicGameState {
        PublicGameState {
            hands: self
                .hands
                .iter()
                .map(|x| x.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            face_up_three: self.face_up_three.iter().map(|x| x.clone().into_boxed_slice()).collect::<Vec<_>>().into_boxed_slice(),
            face_down_three: self.face_down_three.iter().map(|x| x.len() as u8).collect::<Vec<_>>().into_boxed_slice(),
            top_card: self.pile_cards.last().cloned(),
            pile_size: self.pile_cards.len(),
            cleared_size: self.cleared_cards.len(),
            cur_phase: self.cur_phase,
            active_player: self.active_player,
            last_cards_played: self.last_cards_played.clone().into_boxed_slice(),
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
        self.hands[self.active_player as usize] = new_hand;
        self.rotate_play();

        if self.active_player == 0 {
            self.cur_phase = GamePhase::Play;
        }

        Ok(())
    }

    pub fn make_play(&mut self, cards: Box<[Card]>) -> Result<(), &'static str> {
        if cards.len() == 0 {
            return Err("Have to play at least one card")
        }

        if cards.windows(2).filter(|cards| cards[0].value != cards[1].value).count() != 0 {
            return Err("Can only play multiple cards if each card has the same value")
        }

        let card_value = cards[0].value;

        let is_playable = match (card_value, self.effective_top_card()) {
            (CardValue::Two, _) => true,
            (CardValue::Four, _) => true,
            (CardValue::Ten, y @ _) => y != CardValue::Seven,
            (x @ _, y @ _) => x >= y
        };

        // Determine which zone we are playing from
        // move cards from that zone to pile
        if self.hands[self.active_player as usize].len() != 0 {
            // Play from hand
        } else if self.face_up_three[self.active_player as usize].len() != 0 {
            // Play from face up three
        } else {
            // Play from face down three
        }

        self.last_cards_played.clear();
        self.last_cards_played.extend_from_slice(&cards);


        if !is_playable {
            self.hands[self.active_player as usize].extend_from_slice(&self.pile_cards);
            self.pile_cards.clear();
        }

        if card_value == CardValue::Ten || self.top_n_cards_same() {
            self.cleared_cards.extend_from_slice(&self.pile_cards);
            self.pile_cards.clear();
        } else {
            self.rotate_play();
        }

        Ok(())
    }

    fn top_n_cards_same(&self) -> bool {
        // TODO: FIX FOR FOURS
        let top_value = self.effective_top_card();
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

    fn rotate_play(&mut self) {
        self.active_player += 1;
        if self.active_player == self.num_players {
            self.active_player = 0;
        }
    }

    fn effective_top_card(&self) -> CardValue {
        let mut index = self.pile_cards.len() - 1;
        let effective_top_card_value = if let Some(card) = self.pile_cards.get(index) {
            card.value
        } else {
            CardValue::Two
        };
        while effective_top_card_value == CardValue::Four {
            index -= 1;
            let effective_top_card_value = if let Some(card) = self.pile_cards.get(index) {
                card.value
            } else {
                CardValue::Two
            };
        }
        effective_top_card_value
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
    last_cards_played: Box<[Card]>
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

    #[test]
    fn test_enum_order() {
        use super::*;

        assert!(CardValue::Ace > CardValue::Two);
    }
}
