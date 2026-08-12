//! Lightweight verbose-progress helper for timing multi-step operations.

use std::time::{Duration, Instant};

/// On/off flag for verbose progress output.
#[derive(Clone, Copy, Debug, Default)]
pub struct Verbose(pub bool);

impl Verbose {
  pub fn enabled(&self) -> bool {
    self.0
  }

  /// Print an informational message when verbose mode is on.
  pub fn log(&self, msg: &str) {
    if self.0 {
      eprintln!("[docq] {msg}");
    }
  }

  /// Print the elapsed time for a completed step.
  pub fn step(&self, name: &str, elapsed: Duration) {
    if self.0 {
      eprintln!("[docq] {name}: {} ms", elapsed.as_millis());
    }
  }

  /// Start a timed step; the time is printed when the returned `Step` is dropped.
  pub fn start(&self, name: &'static str) -> Step {
    Step::new(*self, name)
  }
}

/// RAII timer for a single verbose step.
pub struct Step {
  verbose: Verbose,
  name: &'static str,
  start: Instant,
}

impl Step {
  pub fn new(verbose: Verbose, name: &'static str) -> Self {
    if verbose.0 {
      eprintln!("[docq] {name}...");
    }
    Self {
      verbose,
      name,
      start: Instant::now(),
    }
  }
}

impl Drop for Step {
  fn drop(&mut self) {
    self.verbose.step(self.name, self.start.elapsed());
  }
}
