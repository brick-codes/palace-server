pub mod data;

use super::*;

use palace_server;
use parking_lot::Mutex;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use ws::{Handler, Handshake, Message, Sender};

pub struct TestClientInner {
   out: Sender,
   recvd_messages: mpsc::Sender<InMessage>,
   to_send_messages: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

impl Handler for TestClientInner {
   fn on_open(&mut self, _: Handshake) -> ws::Result<()> {
      self.out.timeout(100, ws::util::Token(1))
   }

   fn on_timeout(&mut self, event: ws::util::Token) -> ws::Result<()> {
      if event == ws::util::Token(1) {
         if let Ok(bytes) = self.to_send_messages.lock().try_recv() {
            self.out.send(bytes).unwrap();
         }
         self.out.timeout(100, ws::util::Token(1))
      } else {
         Ok(())
      }
   }

   fn on_message(&mut self, msg: Message) -> ws::Result<()> {
      self
         .recvd_messages
         .send(serde_json::from_slice(&msg.into_data()).unwrap())
         .unwrap();
      Ok(())
   }
}

pub struct TestClient {
   recvd_messages: mpsc::Receiver<InMessage>,
   to_send_messages: mpsc::Sender<Vec<u8>>,
}

impl TestClient {
   pub fn new() -> TestClient {
      let (tx, rx) = mpsc::channel();
      let (tx2, rx2) = mpsc::channel();
      let to_send_messages = Arc::new(Mutex::new(rx2));
      std::thread::spawn(move || {
         ws::connect("ws://127.0.0.1:3013", |out| TestClientInner {
            out,
            recvd_messages: tx.clone(),
            to_send_messages: to_send_messages.clone(),
         }).unwrap();
      });
      TestClient {
         recvd_messages: rx,
         to_send_messages: tx2,
      }
   }

   pub fn send(&mut self, message: OutMessage) {
      self
         .to_send_messages
         .send(serde_json::to_vec(&message).unwrap())
         .unwrap();
   }

   pub fn get(&mut self) -> InMessage {
      self.recvd_messages.recv().unwrap()
   }

   pub fn disconnect(&mut self) {
      self
         .to_send_messages
         .send(Vec::from(
            "This message will be unrecognized, causing the connection to end",
         ))
         .unwrap();
   }

   pub fn new_lobby(&mut self) -> (String, String) {
      self.new_lobby_named("TestLobby")
   }

   pub fn new_lobby_named(&mut self, name: &str) -> (String, String) {
      self.send(OutMessage::NewLobby(NewLobbyMessage {
         player_name: "TestClient",
         lobby_name: name,
         password: "foo",
         max_players: 4,
      }));
      let nlr = self.get();
      match nlr {
         InMessage::NewLobbyResponse(r) => {
            let inner = r.expect("New lobby failed");
            (inner.player_id, inner.lobby_id)
         }
         _ => panic!("Expected new lobby response"),
      }
   }
}

static SERVER_UP: Mutex<bool> = Mutex::new(false);

pub fn ensure_test_server_up() {
   let mut server_up = SERVER_UP.lock();
   if !*server_up {
      std::thread::spawn(move || {
         palace_server::run_server("127.0.0.1:3013");
      });
      // TODO ideally this would be a retry ready check
      std::thread::sleep(Duration::from_secs(5));
      *server_up = true
   }
}
