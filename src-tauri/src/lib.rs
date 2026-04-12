mod api;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub struct AiderProcessState(pub Mutex<Vec<Child>>);

pub struct AiderSession {
    pub target_dir: String,
    pub files: String,
    pub message: String,
    pub chat_language: String,
    pub aider_path: String,
    pub file_encoding: String,
    pub git_path: String,
    pub api_port: u16,
}
pub struct AiderSessionState(pub Mutex<Option<AiderSession>>);

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
    chat_language: String,
    aider_path: String,
    file_encoding: String,
    git_path: String,
    api_port: u16,
    app_handle: tauri::AppHandle,
    session_state: tauri::State<'_, AiderSessionState>,
) {
    // Save the session for potential restarts
    let session = AiderSession {
        target_dir: target_dir.clone(),
        files: files.clone(),
        message: message.clone(),
        chat_language: chat_language.clone(),
        aider_path: aider_path.clone(),
        file_encoding: file_encoding.clone(),
        git_path: git_path.clone(),
        api_port,
    };
    *session_state.0.lock().unwrap() = Some(session);

    spawn_aider_process(&app_handle, target_dir, files, message, chat_language, aider_path, file_encoding, api_port, git_path);
}

pub fn spawn_aider_process(app_handle: &tauri::AppHandle, target_dir: String, files: String, message: String, chat_language: String, aider_path: String, file_encoding: String, api_port: u16, git_path: String) {
    let mut path_parts = aider_path.trim().split_whitespace();
    let program = path_parts.next().unwrap_or("aider");
    let mut command = Command::new(program);
    for arg in path_parts {
        command.arg(arg);
    }
    
    command.current_dir(&target_dir);

    let mut path_env = std::env::var("PATH").unwrap_or_default();
    if !git_path.trim().is_empty() {
        #[cfg(windows)]
        { path_env = format!("{};{}", git_path.trim(), path_env); }
        #[cfg(not(windows))]
        { path_env = format!("{}:{}", git_path.trim(), path_env); }
    }
    command.env("PATH", path_env);
    
    command.env("PYTHONUTF8", "1");
    command.env("AIDER_CHECK_UPDATE", "false");

    if !files.trim().is_empty() {
        for file in files.split_whitespace() {
            command.arg(file);
        }
    }

    command.args([
        "--openai-api-base",
        &format!("http://127.0.0.1:{}/v1", api_port),
        "--openai-api-key",
        "dummy",
        "--model",
        "gpt-4o",
        "--no-stream",
        "--no-auto-commits",
        "--yes",
        "--no-analytics",
    ]);

    let temp_dir = std::env::temp_dir();
    let msg_file_path = temp_dir.join(format!("aider_msg_{}.txt", std::process::id()));

    let enc = file_encoding.trim();
    if enc.eq_ignore_ascii_case("cp932") || enc.eq_ignore_ascii_case("shift_jis") {
        // encoding_rsを使用してUTF-8からCP932(Shift_JIS)に変換。マッピングできない文字は安全に置換される。
        let (cow, _, _) = encoding_rs::SHIFT_JIS.encode(&message);
        let _ = std::fs::write(&msg_file_path, cow.as_ref());
    } else {
        let _ = std::fs::write(&msg_file_path, &message);
    }
    command.arg("--message-file").arg(&msg_file_path);

    if !chat_language.trim().is_empty() {
        command.arg("--chat-language").arg(chat_language.trim());
    }

    let enc = file_encoding.trim();
    if !enc.is_empty() {
        command.arg("--encoding").arg(enc);
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let _ = app_handle.emit("aider_log", format!("=> Executing: {:?}", command));

    let state = app_handle.state::<AiderProcessState>();
    let app_handle_clone = app_handle.clone();

    match command.spawn() {
        Ok(mut child) => {
            if let Some(stdout) = child.stdout.take() {
                let app_handle_clone = app_handle_clone.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    let mut reader = std::io::BufReader::new(stdout);
                    let mut buffer = Vec::new();
                    while let Ok(bytes_read) = reader.read_until(b'\n', &mut buffer) {
                        if bytes_read == 0 { break; }
                        let line = String::from_utf8_lossy(&buffer).trim_end().to_string();
                        let _ = app_handle_clone.emit("aider_log", line);
                        buffer.clear();
                    }
                });
            }
            if let Some(stderr) = child.stderr.take() {
                let app_handle_clone = app_handle_clone.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    let mut reader = std::io::BufReader::new(stderr);
                    let mut buffer = Vec::new();
                    while let Ok(bytes_read) = reader.read_until(b'\n', &mut buffer) {
                        if bytes_read == 0 { break; }
                        let line = String::from_utf8_lossy(&buffer).trim_end().to_string();
                        let _ = app_handle_clone.emit("aider_log", line);
                        buffer.clear();
                    }
                });
            }
            state.0.lock().unwrap().push(child);
        }
        Err(e) => {
            let err_msg = format!("[Error] Failed to spawn aider in '{}': {}", target_dir, e);
            eprintln!("{}", err_msg);
            let _ = app_handle.emit("aider_log", err_msg);
        }
    }
}

#[tauri::command]
async fn reset_aider_state(
    app_state: tauri::State<'_, crate::api::AppState>,
    process_state: tauri::State<'_, AiderProcessState>,
) -> Result<(), String> {
    println!("=> [PromptProxy] Resetting system state and killing Aider processes...");
    app_state.pending_requests.lock().await.clear();
    let mut processes = process_state.0.lock().unwrap();
    for mut child in processes.drain(..) {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Manage the state for background processes
            app.manage(AiderProcessState(Mutex::new(Vec::new())));
            app.manage(AiderSessionState(Mutex::new(None)));
            // Manually initialize AppState
            app.manage(crate::api::AppState {
                pending_requests: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
                prompt_settings: std::sync::Arc::new(tokio::sync::Mutex::new(crate::api::PromptSettings {
                    use_custom: false,
                    custom_edit_prompt: String::new(),
                    custom_ask_prompt: String::new(),
                })),
                server_tx: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            });
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_drag::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            api::respond_to_llm_request,
            launch_aider_batch,
            api::update_prompt_settings,
            api::start_api_server,
            reset_aider_state
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let state = app_handle.state::<AiderProcessState>();
            let mut processes = state.0.lock().unwrap();
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
