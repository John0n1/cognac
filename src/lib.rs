pub mod analyzer;
pub mod desktop;
pub mod detector;
pub mod diagnostics;
pub mod environment;
pub mod installer;
pub mod knowledge;
pub mod model;
pub mod paths;
pub mod planner;
pub mod progress;
pub mod registry;
pub mod runner;
pub mod system;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
