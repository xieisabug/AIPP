#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    if aipp_lib::acp_mcp_bridge::run_if_requested() {
        return;
    }

    aipp_lib::run();
}
