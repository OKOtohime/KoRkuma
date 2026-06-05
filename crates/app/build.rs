fn main() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let manifest = std::path::Path::new(&manifest_dir);
    slint_build::compile_with_config(
        "src/ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_bundled_translations("translations")
            .with_default_translation_context(slint_build::DefaultTranslationContext::None)
            .with_library_paths(std::collections::HashMap::from([
                (
                    "material".to_string(),
                    manifest.join("material-1.0/material.slint"),
                ),
                (
                    "material-icon".to_string(),
                    manifest.join("material-1.0/ui/icons/icons.slint"),
                ),
            ])),
    )
    .unwrap();
}