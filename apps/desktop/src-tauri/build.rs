/// Every §8.5 command, so each window's capability grants only the ones it
/// uses (`allow-<command>`).
const COMMANDS: &[&str] = &[
    "get_sessions",
    "jump_to_session",
    "acknowledge",
    "dismiss",
    "set_hit_regions",
    "request_strip_size",
    "get_settings",
    "update_settings",
    "setup_status",
    "setup_list_monitors",
    "setup_preview_hooks",
    "setup_apply_hooks",
    "setup_remove_hooks",
    "run_health_check",
    "play_test_knock",
];

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri build failed");
}
