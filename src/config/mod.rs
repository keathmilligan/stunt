//! Configuration loading, saving, and validation.

mod model;
pub mod session;
mod storage;

pub use model::{Config, ServerEntry, TunnelForward};
pub use session::{SessionRecord, SessionState, load_sessions, save_sessions};
pub use storage::{load, save};
