//! Built-in features. Each one is a self-contained [`tm_core::Module`]; adding a
//! feature means adding a file here and one line in [`builtin`].

use std::sync::Arc;

use tm_core::Module;

pub mod drops;
pub mod notify;
pub mod points;
pub mod predictions;

/// Every built-in module, in start order.
pub fn builtin() -> Vec<Arc<dyn Module>> {
    vec![
        Arc::new(drops::Drops),
        Arc::new(points::Points),
        Arc::new(predictions::Predictions),
        Arc::new(notify::Notify),
    ]
}
