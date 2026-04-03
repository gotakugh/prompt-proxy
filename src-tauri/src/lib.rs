mod api;
use std::process::Command;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn launch_aider_batch(target_dir: String, files: String, message: String) {
    let mut command = Command::new("aider");
    command.current_dir(&target_dir);

    if !files.trim().is_empty() {
        for file in files.split_whitespace() {
            command.arg(file);
        }
    }

    command.args([
        "--openai-api-base",
        "http://localhost:8080/v1",
        "--openai-api-key",
        "dummy",
        "--model",
        "gpt-4o",
        "--no-stream",
        "--no-auto-commits",
        "--yes",
    ]);

    command.arg("--message").arg(message);

    if let Err(e) = command.spawn() {
        eprintln!("Failed to spawn aider in '{}': {}", target_dir, e);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Start the API server in a background task
            api::init(&app.handle());
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            api::respond_to_llm_request,
            launch_aider_batch
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
