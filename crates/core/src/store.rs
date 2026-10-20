//! The store owns the [`State`], assigns sequence numbers, persists and broadcasts.

use std::sync::Arc;

use chrono::Utc;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::event::{Envelope, Event};
use crate::state::{State, apply_envelope};

const CHANNEL_CAPACITY: usize = 4096;

#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    state: RwLock<State>,
    tx: broadcast::Sender<Arc<Envelope>>,
    db: Db,
}

impl Store {
    pub fn new(db: Db) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                state: RwLock::new(State::default()),
                tx,
                db,
            }),
        }
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    /// Applies, persists and broadcasts an event. Returns its sequence number, or
    /// `None` when the event changed nothing (it is then dropped entirely).
    pub fn emit(&self, event: Event) -> Option<u64> {
        let mut state = self.inner.state.write();
        let env = Envelope {
            seq: state.seq + 1,
            ts: Utc::now(),
            event,
        };
        if !apply_envelope(&mut state, &env) {
            return None;
        }
        self.inner.db.record(&env);
        // Sent while holding the lock: subscribers see events in seq order.
        let _ = self.inner.tx.send(Arc::new(env));
        Some(state.seq)
    }

    pub fn read<R>(&self, f: impl FnOnce(&State) -> R) -> R {
        f(&self.inner.state.read())
    }

    pub fn snapshot(&self) -> State {
        self.inner.state.read().clone()
    }

    /// Subscribes to events. To mirror the state without gaps, subscribe first, then
    /// take a [`snapshot`](Self::snapshot) and skip received events with `seq <= snapshot.seq`.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.inner.tx.subscribe()
    }

    /// Atomic subscribe + snapshot.
    pub fn snapshot_and_subscribe(&self) -> (State, broadcast::Receiver<Arc<Envelope>>) {
        let state = self.inner.state.read();
        (state.clone(), self.inner.tx.subscribe())
    }
}
