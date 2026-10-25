//! Desktop application window and native runtime entry point.

/// Open the empty workspace window without starting a host or remote gateway.
///
/// # Errors
///
/// Returns the native Tauri runtime error if the application cannot initialize.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default().run(tauri::generate_context!())
}
