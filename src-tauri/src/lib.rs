mod api;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

struct AiderProcessState(Mutex<Vec<Child>>);

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn launch_aider_batch(
    target_dir: String,
    files: String,
    message: String,
    state: tauri::State<'_, AiderProcessState>,
) {
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

    match command.spawn() {
        Ok(child) => {
            state.0.lock().unwrap().push(child);
        }
        Err(e) => {
            eprintln!("Failed to spawn aider in '{}': {}", target_dir, e);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Manage the state for background processes
            app.manage(AiderProcessState(Mutex::new(Vec::new())));
            // Start the API server in a background task
            api::init(&app.handle());
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_drag::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            api::respond_to_llm_request,
            launch_aider_batch
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let mut processes = app_handle.state::<AiderProcessState>().0.lock().unwrap();
            for mut child in processes.drain(..) {
                let pid = child.id();

                // 1. Ctrl+C と同じ割り込みシグナル(SIGINT)を送る
                #[cfg(unix)]
                let _ = std::process::Command::new("kill")
                    .args(["-INT", &pid.to_string()])
                    .status();
                #[cfg(windows)]
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string()])
                    .status();

                // 2. 最大2秒間、Graceful Shutdown を待機する
                let mut exited = false;
                for _ in 0..20 {
                    if let Ok(Some(_)) = child.try_wait() {
                        exited = true;
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }

                // 3. それでも終了していなければ強制終了（KILL）
                if !exited {
                    println!("プロセス {} がタイムアウトしました。強制終了します...", pid);
                    let _ = child.kill();
                }
                // 最後にゾンビプロセス回収のためにwait
                let _ = child.wait();
            }
        }
    });
}
