//! Twitch protocol layer for TwitchMiner.
//!
//! Stateless building blocks only: no scheduling, no persistence. The engine in
//! `tm-core` composes them.

pub mod api;
pub mod auth;
pub mod error;
pub mod gql;
pub mod manifest;
pub mod models;
pub mod net;
pub mod pubsub;
pub mod tls;
pub mod watch;
pub mod ws;

pub use error::{Error, Result};
pub use gql::Gql;
pub use manifest::{Manifest, SharedManifest};
pub use net::{Net, NetConfig};
pub use pubsub::{PubSub, PubSubEvent, PubSubMessage, TopicSet};
