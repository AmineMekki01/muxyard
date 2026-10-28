//! Desktop application window and typed host bridge commands.

mod commands;
mod host_bridge;

use tauri::Manager;

pub use commands::{
    host_connect, host_status, host_unwatch, host_watch, project_list, project_register,
    project_remove, terminal_attach, terminal_close, terminal_input, terminal_render_config,
    terminal_resize, terminal_snapshot, terminal_spawn, workspace_layout_get,
    workspace_layout_save,
};

/// Open the workspace window and register the typed local host bridge.
///
/// The bridge launches a host when absent but never stops or owns it. Closing
/// the desktop window therefore leaves an already-running host untouched.
///
/// # Errors
///
/// Returns the native Tauri runtime error if the application cannot initialize.
pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .manage(commands::HostWatchState::default())
        .invoke_handler(tauri::generate_handler![
            host_connect,
            host_status,
            host_watch,
            host_unwatch,
            project_list,
            project_register,
            project_remove,
            terminal_render_config,
            terminal_spawn,
            terminal_snapshot,
            terminal_input,
            terminal_resize,
            terminal_close,
            terminal_attach,
            workspace_layout_get,
            workspace_layout_save
        ])
        .build(tauri::generate_context!())?;
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            app_handle.state::<commands::HostWatchState>().stop();
        }
    });
    Ok(())
}
