fn main() {
    slint_build::compile_with_config(
        "src/ui.slint",
        slint_build::CompilerConfiguration::new()
            .with_bundled_translations("translations")
            .with_default_translation_context(
                slint_build::DefaultTranslationContext::None,
            ),
    )
    .unwrap();
}