mod api;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

pub struct AiderProcessState(pub Mutex<Vec<u32>>);

pub struct AiderSession {
    pub target_dir: String,
    pub files: String,
    pub message: String,
    pub chat_language: String,
    pub aider_path: String,
    pub file_encoding: String,
    pub git_path: String,
    pub map_tokens: String,
    pub output_extension: String,
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
    map_tokens: String,
    output_extension: String,
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
        map_tokens: map_tokens.clone(),
        output_extension: output_extension.clone(),
        api_port,
    };
    *session_state.0.lock().unwrap() = Some(session);

    spawn_aider_process(&app_handle, target_dir, files, message, chat_language, aider_path, file_encoding, api_port, git_path, map_tokens);
}

pub fn resolve_encoding_labels(input: &str) -> (String, String) {
    let normalized = input.trim().to_lowercase();
    match normalized.as_str() {
        "cp932" | "windows-31j" | "shift_jis" | "sjis" => {
            // Rust(WHATWG) prefers windows-31j, Python prefers cp932
            ("windows-31j".to_string(), "cp932".to_string())
        },
        "euc-jp" | "euc_jp" => {
            ("euc-jp".to_string(), "euc_jp".to_string())
        },
        _ => {
            // Pass through unknown or utf-8 as-is
            (normalized.clone(), normalized)
        }
    }
}

pub fn spawn_aider_process(app_handle: &tauri::AppHandle, target_dir: String, _files: String, message: String, chat_language: String, aider_path: String, file_encoding: String, api_port: u16, git_path: String, map_tokens: String) {
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

    command.args([
        "--openai-api-base",
        &format!("http://127.0.0.1:{}/v1", api_port),
        "--openai-api-key",
        "dummy",
        "--model",
        "gpt-4o",
        "--edit-format", // NEW: Disable Aider's default formatting rules
        "ask",           // NEW: Generate plain prompts without rules
        "--no-stream",
        "--no-auto-commits",
        "--yes",
        "--no-analytics",
        "--no-show-release-notes",
        "--no-check-update",
    ]);

    if !map_tokens.trim().is_empty() {
        command.arg("--map-tokens").arg(map_tokens.trim());
    }

    let temp_dir = std::env::temp_dir();
    let msg_file_path = temp_dir.join(format!("aider_msg_{}.txt", std::process::id()));

    let enc_input = file_encoding.trim();
    let (rust_enc, python_enc) = resolve_encoding_labels(enc_input);

    if !rust_enc.is_empty() {
        if let Some(encoding) = encoding_rs::Encoding::for_label(rust_enc.as_bytes()) {
            let (cow, _, _) = encoding.encode(&message);
            let _ = std::fs::write(&msg_file_path, cow.as_ref());
        } else {
            let _ = std::fs::write(&msg_file_path, &message);
        }
    } else {
        let _ = std::fs::write(&msg_file_path, &message);
    }
    command.arg("--message-file").arg(&msg_file_path);

    if !chat_language.trim().is_empty() {
        command.arg("--chat-language").arg(chat_language.trim());
    }

    if !python_enc.is_empty() {
        command.arg("--encoding").arg(python_enc);
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let _ = app_handle.emit("aider_log", format!("=> Executing: {:?}", command));

    let state = app_handle.state::<AiderProcessState>();
    let app_handle_clone = app_handle.clone();

    match command.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            state.0.lock().unwrap().push(pid);

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
            
            // Monitor process termination and emit events
            let app_handle_clone = app_handle.clone();
            std::thread::spawn(move || {
                let status = child.wait().unwrap();
                let _ = app_handle_clone.emit("aider_log", format!("--- Operation Finished ({}) ---", status));
                let _ = app_handle_clone.emit("aider_finished", status.success());

                // Remove PID from state
                let state = app_handle_clone.state::<AiderProcessState>();
                let mut pids = state.0.lock().unwrap();
                pids.retain(|&p| p != pid);
            });
        }
        Err(e) => {
            let err_msg = format!("[Error] Failed to spawn aider in '{}': {}", target_dir, e);
            eprintln!("{}", err_msg);
            let _ = app_handle.emit("aider_log", err_msg);
        }
    }
}

#[tauri::command]
async fn apply_patch(
    target_dir: String,
    response: String,
    aider_path: String,
    file_encoding: String,
    git_path: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut processed_response = response.clone();
    let mut target_file_path = None;
    let lines: Vec<&str> = response.lines().collect();
    
    for (i, line) in lines.iter().enumerate() {
        if line.contains("<<<<<<< SEARCH") {
            // Search upwards from the SEARCH line for a line that isn't empty or a markdown block start (```)
            for j in (0..i).rev() {
                let prev_line = lines[j].trim();
                if !prev_line.is_empty() && !prev_line.starts_with("```") {
                    // Strip decorations like backticks or asterisks
                    let clean_path = prev_line.trim_matches(|c| c == '`' || c == '*' || c == '"' || c == '\'');
                    target_file_path = Some(clean_path);
                    break;
                }
            }
            break;
        }
    }

    // Default to OS-specific line endings (CRLF for Windows, LF for Mac/Linux)
    let mut use_crlf = cfg!(windows);
    
    if let Some(rel_path) = target_file_path {
        let full_path = std::path::Path::new(&target_dir).join(rel_path);
        
        // If the file exists, prioritize and use its actual line endings
        if let Ok(bytes) = std::fs::read(&full_path) {
            if bytes.windows(2).any(|w| w == b"\r\n") {
                use_crlf = true;
            } else if bytes.contains(&b'\n') {
                use_crlf = false; // Unix line ending (LF) detected!
            }
        } else {
            let _ = app_handle.emit("aider_log", format!("=> [PromptProxy] Note: Could not read target file for line-ending detection. Defaulting to OS standard. Path: {:?}", full_path));
        }
    }

    if use_crlf {
        processed_response = processed_response.replace("\r\n", "\n").replace("\n", "\r\n");
    } else {
        processed_response = processed_response.replace("\r\n", "\n");
    }

    let temp_dir = std::env::temp_dir();
    let patch_file_path = temp_dir.join(format!("aider_patch_{}.txt", std::process::id()));

    let enc_input = file_encoding.trim();
    let (rust_enc, python_enc) = resolve_encoding_labels(enc_input);

    if !rust_enc.is_empty() {
        if let Some(encoding) = encoding_rs::Encoding::for_label(rust_enc.as_bytes()) {
            let (cow, _, _) = encoding.encode(&processed_response);
            let _ = std::fs::write(&patch_file_path, cow.as_ref());
        } else {
            let _ = std::fs::write(&patch_file_path, &processed_response);
        }
    } else {
        let _ = std::fs::write(&patch_file_path, &processed_response);
    }

    let mut path_parts = aider_path.trim().split_whitespace();
    let program = path_parts.next().unwrap_or("aider");
    let mut command = std::process::Command::new(program);
    for arg in path_parts { command.arg(arg); }

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

    // Retrieve the local port that PromptProxy is listening on from AiderSessionState
    let api_port = {
        let state_guard = app_handle.state::<crate::AiderSessionState>();
        let lock = state_guard.0.lock().unwrap();
        lock.as_ref().map(|s| s.api_port).unwrap_or(8080)
    };

    // Override initialization communication to local by passing dummy settings in arguments instead of using env_remove
    command.args([
        "--openai-api-base",
        &format!("http://127.0.0.1:{}/v1", api_port),
        "--openai-api-key",
        "dummy",
        "--model",
        "gpt-4o",
    ]);

    command.arg("--apply").arg(&patch_file_path);
    command.arg("--yes");
    command.arg("--no-analytics");
    command.arg("--no-auto-commits"); // Add: Block auto-commits and API communication after patch application
    command.arg("--no-show-release-notes");
    command.arg("--no-check-update");

    if !python_enc.is_empty() {
        command.arg("--encoding").arg(python_enc);
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let _ = app_handle.emit("aider_log", format!("=> Executing Apply: {:?}", command));

    let app_handle_clone = app_handle.clone();
    match command.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            let state = app_handle.state::<crate::AiderProcessState>();
            state.0.lock().unwrap().push(pid);

            if let Some(stdout) = child.stdout.take() { /* Existing stdout thread */
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
            if let Some(stderr) = child.stderr.take() { /* Existing stderr thread */
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

            // Monitor process termination
            let app_handle_clone = app_handle.clone();
            std::thread::spawn(move || {
                let status = child.wait().unwrap();
                let _ = app_handle_clone.emit("aider_log", format!("--- Patch Applied ({}) ---", status));
                let _ = app_handle_clone.emit("aider_finished", status.success());

                let state = app_handle_clone.state::<crate::AiderProcessState>();
                let mut pids = state.0.lock().unwrap();
                pids.retain(|&p| p != pid);
            });
        }
        Err(e) => {
            let _ = app_handle.emit("aider_log", format!("[Error] Failed to apply patch: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
async fn reset_aider_state(
    process_state: tauri::State<'_, AiderProcessState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let _ = app_handle.emit("aider_log", "=> [PromptProxy] Aborting current operation...".to_string());
    let mut pids = process_state.0.lock().unwrap();
    for pid in pids.drain(..) {
        #[cfg(unix)]
        let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).status();
        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status();
    }
    let _ = app_handle.emit("aider_finished", false);
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
            launch_aider_batch,
            api::update_prompt_settings,
            api::start_api_server,
            reset_aider_state,
            apply_patch,
            api::pack_target_files,
            api::get_directory_files,
            api::load_config,
            api::save_config,
            api::open_config_dir
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let state = app_handle.state::<AiderProcessState>();
            let mut pids = state.0.lock().unwrap();
            for pid in pids.drain(..) {
                #[cfg(unix)]
                let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).status();
                #[cfg(windows)]
                let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).status();
            }
        }
    });
}
