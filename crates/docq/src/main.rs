//! docq CLI entry point.

mod engine;

pub use engine::{Engine, EngineConfig};

fn main() {
  println!("docq v0.1 — use `docq init`, `docq add`, `docq index`, `docq search`, `docq ask`");
}
