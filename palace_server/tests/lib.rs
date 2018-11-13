#![feature(custom_test_frameworks)]
#![test_runner(crate::testrunner)]

mod common;

use crate::common::data::*;
use crate::common::*;
use std::time::Duration;
use timebomb::timeout_ms;

fn testrunner(cases: &[&dyn Fn()]) {
   env_logger::init();
   std::thread::spawn(move || {
      palace_server::run_server("127.0.0.1:3013");
   });
   // TODO ideally this would be a retry ready check
   std::thread::sleep(Duration::from_secs(5));
   // TODO: we should run these in parallel (rayon)
   for t in cases {
      t();
   }
   println!("executed {} tests", cases.len());
}

#[test_case]
fn lobbies_clean_up() {
   const JUNK_LOBBY_NAME: &str = "JunkLobby";

   // Create a lobby
   {
      let mut tc = TestClient::new();
      tc.new_lobby_named(JUNK_LOBBY_NAME);
      tc.disconnect();
   }

   std::thread::sleep(Duration::from_secs(30));

   // Ensure lobby is cleaned up
   {
      let mut tc = TestClient::new();
      tc.send(&OutMessage::ListLobbies(ListLobbiesMessage {
         page: 0,
      }));
      let llr = tc.get();
      match llr {
         InMessage::ListLobbiesResponse(r) => assert!(r.lobbies.iter().find(|x| x.name == JUNK_LOBBY_NAME).is_none()),
         _ => panic!("Expected list lobbies response"),
      }
   }
}

#[test_case]
fn bots_join_lobby_after_request() {
   let mut tc = TestClient::new();
   // Create a lobby
   let (player_id, lobby_id) = tc.new_lobby();

   // Request 3 AI
   {
      tc.send(&OutMessage::RequestAi(RequestAiMessage {
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

#[test_case]
fn kicking_player_new_player_reuse_id() {
   let mut tc = TestClient::new();
   let (player_id, lobby_id) = tc.new_lobby();

   // Request some AI
   {
      tc.send(&OutMessage::RequestAi(RequestAiMessage {
         num_ai: 3,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that three AI join
      for i in 0..3 {
         match tc.get() {
            InMessage::PlayerJoinEvent(pje) => {
               assert_eq!(pje.slot, i + 1);
            }
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
      tc.send(&OutMessage::KickPlayer(KickPlayerMessage {
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
         _ => panic!("Expected KickPlayerResponse"),
      }
   }

   // Request an AI, make sure it fills the empty slot
   {
      tc.send(&OutMessage::RequestAi(RequestAiMessage {
         num_ai: 1,
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));

      // Ensure that one AI joins
      match tc.get() {
         InMessage::PlayerJoinEvent(pje) => {
            assert_eq!(pje.slot, 2);
         }
         _ => panic!("Expected PlayerJoinEvent"),
      }

      // Ensure the AI response is OK
      match tc.get() {
         InMessage::RequestAiResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected RequestAiResponse"),
      }
   }
}

#[test_case]
fn owner_leaving_closes_lobby() {
   let mut owner_tc = TestClient::new();
   let mut player_tc = TestClient::new();

   let (_, lobby_id) = owner_tc.new_lobby();

   // Join lobby
   {
      player_tc.send(&OutMessage::JoinLobby(JoinLobbyMessage {
         lobby_id: &lobby_id,
         player_name: "TestClient",
         password: "",
      }));
      match player_tc.get() {
         InMessage::JoinLobbyResponse(r) => assert!(r.is_ok()),
         _ => panic!("Expected JoinLobbyResponse"),
      }
   }

   // Have owner leave
   let _ = owner_tc.get(); // PlayerJoinEvent
   owner_tc.disconnect();

   // Lobby should have closed
   {
      match player_tc.get() {
         InMessage::LobbyCloseEvent(lce) => assert_eq!(lce, LobbyCloseEvent::OwnerLeft),
         x => panic!("Expected LobbyCloseEvent, got {:?}", x),
      }
   }
}

#[test_case]
fn afk_kick() {
   let mut tc = TestClient::new();

   // Create lobby
   let (player_id, lobby_id) = tc.new_lobby();

   // Add AI
   {
      tc.send(&OutMessage::RequestAi(RequestAiMessage {
         player_id: &player_id,
         lobby_id: &lobby_id,
         num_ai: 1,
      }));
      let _ = tc.get(); // PJE
      let _ = tc.get(); // RAR
   }

   // Start game
   {
      tc.send(&OutMessage::StartGame(StartGameMessage {
         player_id: &player_id,
         lobby_id: &lobby_id,
      }));
   }

   timeout_ms(
      move || loop {
         match tc.get() {
            InMessage::LobbyCloseEvent(reason) => {
               assert!(reason == LobbyCloseEvent::Afk);
               break;
            }
            _ => continue,
         }
      },
      60000,
   );
}

#[test_case]
fn clandestine_bots_join_lobby() {
   let mut tc = TestClient::new();

   let (_player_id, _lobby_id) = tc.new_lobby();

   timeout_ms(
      move || loop {
         match tc.get() {
            InMessage::PlayerJoinEvent(_) => {
               break;
            }
            _ => assert!(false),
         }
      },
      21000,
   );
}
