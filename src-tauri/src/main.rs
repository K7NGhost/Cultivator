// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if cultivator_lib::is_search_worker_process() {
        std::process::exit(cultivator_lib::run_search_worker_stdio());
    }

    cultivator_lib::run()
}
