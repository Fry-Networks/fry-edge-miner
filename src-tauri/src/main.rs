#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod commands;
mod config;
mod integrations;
mod poc;
mod supervisor;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::integration::get_integrations,
            commands::integration::toggle_integration,
            commands::device::get_device_info,
            commands::rewards::get_rewards,
            commands::settings::get_settings,
            commands::settings::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FEM")
}
