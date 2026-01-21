//! Connecto GUI - Tauri Application Entry Point
//!
//! A modern GUI for SSH key pairing using Tauri.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod state;

use commands::{
    delete_local_key, generate_key_pair, get_addresses, get_device_name, get_key_details,
    get_listener_status, list_authorized_keys, list_local_keys, list_paired_hosts,
    pair_with_address, pair_with_device, remove_authorized_key, rename_local_key, scan_devices,
    start_listener, stop_listener,
};
use state::AppState;
use tracing_subscriber::EnvFilter;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_device_name,
            get_addresses,
            scan_devices,
            pair_with_device,
            pair_with_address,
            start_listener,
            stop_listener,
            get_listener_status,
            list_authorized_keys,
            remove_authorized_key,
            generate_key_pair,
            list_paired_hosts,
            list_local_keys,
            delete_local_key,
            get_key_details,
            rename_local_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
