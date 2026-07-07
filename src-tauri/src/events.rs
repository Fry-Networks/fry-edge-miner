//! Global app-handle holder so non-Tauri modules (docker_manager, health
//! loops) can emit UI events without threading an AppHandle through every
//! call chain. Emits are silently dropped before setup registers the handle.

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(handle: AppHandle) {
    let _ = HANDLE.set(handle);
}

pub fn emit<S: serde::Serialize + Clone>(event: &str, payload: S) {
    if let Some(handle) = HANDLE.get() {
        if let Err(e) = handle.emit(event, payload) {
            tracing::warn!(event = event, error = %e, "Failed to emit event");
        }
    }
}
