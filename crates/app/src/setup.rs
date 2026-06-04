/// Normalise `LANG` to `en_US.UTF-8` when the detected language has no bundled ICU4X
/// segmentation model in Slint. Must be called before Slint or ICU4X initialise.
pub fn normalize_lang_for_slint() {
    let lang = std::env::var("LANG").unwrap_or_default();
    let tag = lang.split('.').next().unwrap_or("").to_ascii_lowercase();
    let lang_code = tag.split('_').next().unwrap_or("");
    // ICU4X bundles segmentation models for Latin-script languages; CJK and others that rely
    // on ML-based word-breaking are not included in Slint's data bundle.
    let needs_ml_segmenter = matches!(lang_code, "ja" | "zh" | "th" | "km" | "lo" | "my");
    if needs_ml_segmenter {
        // SAFETY: called before Slint initialises its platform and before any threads are spawned.
        unsafe { std::env::set_var("LANG", "en_US.UTF-8"); }
    }
}

pub fn system_locale() -> String {
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LANGUAGE"))
        .or_else(|_| std::env::var("LC_ALL"))
        .unwrap_or_else(|_| "en".to_string());
    raw.split('.').next().unwrap_or(&raw).replace('_', "-")
}

pub fn select_backend() -> &'static str {
    if std::env::var("SLINT_BACKEND").is_ok() {
        return "custom (SLINT_BACKEND env)";
    }
    let backend = if hardware_gl_available() { "winit-femtovg" } else { "winit-software" };
    // SAFETY: called before Slint initialisation and before spawning any threads
    unsafe { std::env::set_var("SLINT_BACKEND", backend); }
    backend
}

fn hardware_gl_available() -> bool {
    if std::env::var("LIBGL_ALWAYS_SOFTWARE").ok().as_deref() == Some("1") {
        return false;
    }
    if std::env::var("GALLIUM_DRIVER")
        .ok()
        .map(|d| matches!(d.as_str(), "llvmpipe" | "softpipe" | "swr"))
        .unwrap_or(false)
    {
        return false;
    }
    platform_has_hw_gl()
}

#[cfg(target_os = "linux")]
fn platform_has_hw_gl() -> bool {
    let has_gpu = std::path::Path::new("/dev/dri/renderD128").exists()
        || std::path::Path::new("/dev/dri/card0").exists();
    has_gpu && (std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok())
}

#[cfg(target_os = "windows")]
fn platform_has_hw_gl() -> bool {
    true
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_has_hw_gl() -> bool {
    true
}