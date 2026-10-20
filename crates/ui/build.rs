fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        // Plain msgids (no per-component context): one French string per English string.
        .with_default_translation_context(slint_build::DefaultTranslationContext::None)
        .with_bundled_translations(concat!(env!("CARGO_MANIFEST_DIR"), "/translations"));
    slint_build::compile_with_config("ui/app.slint", config).expect("slint build failed");
}
