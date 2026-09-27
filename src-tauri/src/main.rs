// Tauri entry point — delegates to the library crate.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    droidbridge_lib::run();
}
