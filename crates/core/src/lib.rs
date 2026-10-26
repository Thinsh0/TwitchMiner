//! TwitchMiner engine.
//!
//! - [`Store`] holds the single [`State`]; every change is an [`Event`].
//! - [`Engine`] runs accounts and [`Module`]s and executes [`Command`]s.
//! - Everything persistent lives in one data directory ([`Paths`]), in one SQLite file.
//!
//! Front-ends (desktop, Android, the HTTP server, bots…) only need three calls:
//! `engine.store().snapshot_and_subscribe()`, and `engine.execute(command)`.

mod account;
pub mod command;
pub mod db;
pub mod engine;
pub mod event;
pub mod guests;
pub mod logging;
pub mod module;
pub mod paths;
mod peers;
pub mod scheduler;
pub mod settings;
pub mod state;
pub mod store;
pub mod view;

pub use command::{ApiError, ApiResult, Command};
pub use engine::{Engine, EngineConfig};
pub use event::{Envelope, Event};
pub use module::{Module, ModuleCall, ModuleContext, ModuleInfo};
pub use paths::Paths;
pub use scheduler::{WatchRequest, priority};
pub use state::State;
pub use store::Store;

pub use tm_twitch as twitch;
