extern crate palace_server;
extern crate parking_lot;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate ws;

mod common;

use common::data::*;
use common::*;
use std::time::Duration;

#[test]
fn test_lobbies_clean_up() {
   ensure_test_server_up();

   // Create a lobby
   {
      let mut tc = TestClient::new();
      tc.send(OutMessage::NewLobby(NewLobbyMessage {
         player_name: "TestClient".into(),
         lobby_name: "JunkLobby".into(),
         password: "".into(),
         max_players: 4,
      }));
      let nlr = tc.get();
      match nlr {
         InMessage::NewLobbyResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected new lobby response"),
      }
      tc.disconnect();
   }

   std::thread::sleep(Duration::from_secs(30));

   // Ensure lobby is cleaned up
   {
      let mut tc = TestClient::new();
      tc.send(OutMessage::ListLobbies);
      let llr = tc.get();
      match llr {
         InMessage::ListLobbiesResponse(r) => assert!(r.iter().find(|x| x.name == "JunkLobby").is_none()),
         _ => panic!("Expected list lobbies response"),
      }
   }
}

#[test]
fn test_bots_join_lobby_after_request() {
   ensure_test_server_up();

   let mut tc = TestClient::new();
   // Create a lobby
   let (player_id, lobby_id) = {
      tc.send(OutMessage::NewLobby(NewLobbyMessage {
         player_name: "TestClient".into(),
         lobby_name: "TestLobby".into(),
         password: "foo".into(),
         max_players: 4,
      }));
      let nlr = tc.get();
      match nlr {
         InMessage::NewLobbyResponse(r) => {
            let inner = r.expect("New lobby failed");
            (inner.player_id, inner.lobby_id)
         }
         _ => panic!("Expected new lobby response"),
      }
   };

   // Request 3 AI
   {
      tc.send(OutMessage::RequestAi(RequestAiMessage {
         num_ai: 3,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that three AI join
      for _ in 0..3 {
         match tc.get() {
            InMessage::PlayerJoinEvent(_) => (),
            _ => panic!("Expected PlayerJoinEvent"),
         }
      }

      // Ensure the AI response is OK
      match tc.get() {
         InMessage::RequestAiResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected RequestAiResponse"),
      }
   }
}

#[test]
fn test_kicking_player_new_player_reuse_id() {
   ensure_test_server_up();

   let mut tc = TestClient::new();
   let (player_id, lobby_id) = {
      tc.send(OutMessage::NewLobby(NewLobbyMessage {
         player_name: "TestClient".into(),
         lobby_name: "TestLobby".into(),
         password: "foo".into(),
         max_players: 4,
      }));
      let nlr = tc.get();
      match nlr {
         InMessage::NewLobbyResponse(r) => {
            let inner = r.expect("New lobby failed");
            (inner.player_id, inner.lobby_id)
         }
         _ => panic!("Expected new lobby response"),
      }
   };

   // Request some AI
   {
      tc.send(OutMessage::RequestAi(RequestAiMessage {
         num_ai: 3,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that three AI join
      for i in 0..3 {
         match tc.get() {
            InMessage::PlayerJoinEvent(pje) => {
               assert_eq!(pje.slot, i + 1);
            },
            _ => panic!("Expected PlayerJoinEvent"),
         }
      }

      // Ensure the AI response is OK
      match tc.get() {
         InMessage::RequestAiResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected RequestAiResponse"),
      }
   }

   // Kick player
   {
      tc.send(OutMessage::KickPlayer(KickPlayerMessage {
         slot: 2,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that slot got kicked
      match tc.get() {
         InMessage::PlayerLeaveEvent(ple) => assert_eq!(ple.slot, 2),
         _ => panic!("Expected PlayerLeaveEvent"),
      }

      // Ensure the kick response is OK
      match tc.get() {
         InMessage::KickPlayerResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected KickPlayerResponse")
      }
   }

   // Request an AI, make sure it fills the empty slot
   {
      tc.send(OutMessage::RequestAi(RequestAiMessage {
         num_ai: 1,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that one AI joins
      match tc.get() {
         InMessage::PlayerJoinEvent(pje) => {
            assert_eq!(pje.slot, 2);
         },
         _ => panic!("Expected PlayerJoinEvent"),
      }

      // Ensure the AI response is OK
      match tc.get() {
         InMessage::RequestAiResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected RequestAiResponse"),
      }
   }
}
