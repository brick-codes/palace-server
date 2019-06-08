mod ai_tournament;

fn main() {
   pretty_env_logger::init();
   if std::env::args().any(|x| x == "--ai") {
      ai_tournament::go();
   } else if std::env::args().any(|x| x == "--monty") {
      ai_tournament::monty_report();
   } else {
      palace_server::run_server("0.0.0.0:3012");
   }
}
