extern crate palace_server;
extern crate pretty_env_logger;

fn main() {
   pretty_env_logger::init();
   palace_server::run_server("0.0.0.0:3012")
}
