#[tauri::command]
fn backend_version() -> &'static str {
    codex_plus_core::version::VERSION
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![backend_version])
        .run(tauri::generate_context!())
        .expect("failed to run Codex++ manager");
}
