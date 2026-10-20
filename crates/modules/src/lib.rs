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

#[cfg(test)]
mod tests {
    #[test]
    fn every_module_has_a_settings_schema_matching_its_defaults() {
        for m in super::builtin() {
            let info = m.info();
            assert!(
                info.settings_schema.is_object(),
                "{} has no schema",
                info.id
            );
            assert!(
                info.settings_schema["properties"]["enabled"].is_object(),
                "{}",
                info.id
            );
            m.validate_settings(&info.default_settings)
                .unwrap_or_else(|e| panic!("{}: {e}", info.id));
        }
    }
}
