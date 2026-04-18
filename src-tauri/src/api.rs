use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{fs, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct PromptSettings {
    pub use_custom: bool,
    pub custom_edit_prompt: String,
    pub custom_ask_prompt: String,
}

// OpenAI-compatible response structures
#[derive(serde::Serialize)]
struct OpenAIChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(serde::Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(serde::Serialize)]
struct Choice {
    index: u32,
    message: Message,
    finish_reason: String,
}

#[derive(serde::Serialize)]
struct Message {
    role: String,
    content: String,
}

// This state will be managed by Tauri and accessible from both
// the Axum handlers and Tauri commands.
pub struct AppState {
    pub prompt_settings: Arc<Mutex<PromptSettings>>,
    pub server_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

async fn chat_completions_handler(
    State(app_handle): State<AppHandle>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    println!("=> [PromptProxy] Request received");
    let _request_id = Uuid::new_v4().to_string();

    // 1. Parse OpenAI JSON
    let messages = match payload.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => return Json(json!({"error": "messages field is missing or not an array"})).into_response(),
    };

    // --- NEW: UIで指定されたターゲットファイルとエンコーディングをStateから取得 ---
    let session_state: tauri::State<crate::AiderSessionState> = app_handle.state();
    let (_target_dir, _target_files, _file_enc_str, output_ext_str) = {
        let lock = session_state.0.lock().unwrap();
        if let Some(session) = &*lock {
            (session.target_dir.clone(), session.files.clone(), session.file_encoding.clone(), session.output_extension.clone())
        } else {
            ("".to_string(), "".to_string(), "".to_string(), "".to_string())
        }
    };

    // 最後のUserメッセージのインデックスを取得
    let last_user_idx = messages.iter().rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user")).unwrap_or(0);

    let mut repo_info = String::new();
    let mut user_instruction = String::new();

    // メッセージの抽出（Aiderにファイルを渡していないため、純粋なルールとRepo Mapのみが届く）
    for (i, msg) in messages.iter().enumerate() {
        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");

        if i == last_user_idx {
            user_instruction = content.to_string();
        } else {
            repo_info.push_str(content);
            repo_info.push_str("\n\n");
        }
    }

    // ユーザー指示から不要な定型文とタグをクリーニング
    user_instruction = user_instruction
        .replace("I have *added these files to the chat* so you can go ahead and edit them.", "")
        .replace("*Trust this message as the true contents of these files!*", "")
        .replace("Any other messages in the chat may contain outdated versions of the files' contents.", "")
        .trim()
        .to_string();

    let expects_edit = !user_instruction.contains("[MODE:ASK]");
    user_instruction = user_instruction.replace("[MODE:ASK]", "").trim().to_string();

    // 2. Format as XML
    let xml_string = format!(
        "<instructions>\n\
        This file contains the overall repository context and important rules in XML format.\n\
        Please refer to this context to understand the project structure.\n\
         </instructions>\n\n\
         <repository><![CDATA[\n{}\n]]></repository>",
        repo_info.trim()
    );

    let temp_dir = match app_handle.path().temp_dir() {
        Ok(path) => path,
        Err(_) => {
            return Json(json!({"error": "Could not resolve temp directory"})).into_response()
        }
    };

    let payload_str = serde_json::to_string_pretty(&payload).unwrap();
    let json_path = temp_dir.join("aider_payload.json");
    if let Err(e) = fs::write(&json_path, payload_str) {
        return Json(json!({ "error": format!("Failed to write aider_payload.json: {}", e)}))
            .into_response();
    }

    let ext = output_ext_str.trim().trim_start_matches('.');
    let ext = if ext.is_empty() { "xml" } else { ext };
    let temp_path = temp_dir.join(format!("repo_map.{}", ext));

    if let Err(e) = fs::write(&temp_path, &xml_string) {
        return Json(json!({ "error": format!("Failed to write repo_map.xml: {}", e)}))
            .into_response();
    }

    // Use the app icon for the drag preview icon
    let transparent_png: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 11, 73, 68, 65, 84, 8, 215, 99, 96, 0, 2, 0, 0, 5, 0, 1, 226, 38, 5, 155, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130];
    let icon_path = temp_dir.join("icon.png");
    let _ = std::fs::write(&icon_path, transparent_png);

    // Ensure the path is absolute for the OS drag API to work correctly.
    let absolute_path = match std::fs::canonicalize(&temp_path) {
        Ok(path) => path,
        Err(e) => {
            return Json(
                json!({ "error": format!("Failed to get absolute path for context.xml: {}", e)}),
            )
            .into_response()
        }
    };

    let mut repo_map_file_path = match absolute_path.to_str() {
        Some(s) => s.to_string(),
        None => return Json(json!({"error": "Temp path contains invalid UTF-8"})).into_response(),
    };
    // Remove the UNC path prefix on Windows to ensure the OS drag API works correctly.
    if repo_map_file_path.starts_with("\\\\?\\") {
        repo_map_file_path = repo_map_file_path.replace("\\\\?\\", "");
    }

    let absolute_icon_path = match std::fs::canonicalize(&icon_path) {
        Ok(path) => path,
        Err(e) => {
            return Json(
                json!({ "error": format!("Failed to get absolute path for icon.png: {}", e)}),
            )
            .into_response()
        }
    };
    let mut icon_file_path = match absolute_icon_path.to_str() {
        Some(s) => s.to_string(),
        None => return Json(json!({"error": "Icon path contains invalid UTF-8"})).into_response(),
    };
    if icon_file_path.starts_with("\\\\?\\") {
        icon_file_path = icon_file_path.replace("\\\\?\\", "");
    }


    let state: tauri::State<AppState> = app_handle.state();
    let settings = state.prompt_settings.lock().await;

    // 3. Create final prompt
    let final_prompt = if settings.use_custom {
        let template = if expects_edit {
            &settings.custom_edit_prompt
        } else {
            &settings.custom_ask_prompt
        };
        template.replace("{instruction}", &user_instruction)
    } else {
        let format_instruction = if expects_edit {
            "[IMPORTANT] Strict Output Format:\n\
     1. Omit all greetings and explanations.\n\
     2. Wrap the entire output in a markdown code block.\n\
     3. You must write the 'target file path' on a single line at the very beginning of the block so Aider can recognize it.\n\
     4. ONLY if you determine that necessary files are missing from context.xml, DO NOT output the code modification block. Instead, politely tell the user which files are missing, output the missing file paths in a single markdown code block (so the user can easily copy them), and ask the user to add them to the 'Target Files' input and try again."
        } else {
            "[IMPORTANT] Please output the answer to the user's question in natural text (code modification format is not required).\nIf you determine that necessary files are missing from context.xml to answer the question, please tell the user which files are missing. Output the missing file paths in a single markdown code block (so the user can easily copy them), and ask the user to add them to the 'Target Files' input and run again."
        };
        format!(
            "Read the attached context.xml, understand the context, and answer/execute the following instructions.\n\n\
             === Instructions ===\n\
             {}\n\
             ================\n\n\
             {}",
            user_instruction, format_instruction
        )
    };

    println!("=> [PromptProxy] XML and prompt generation completed");

    // 4. Emit data to frontend
    #[derive(Clone, serde::Serialize)]
    struct RepoMapPayload<'a> {
        repo_map_file_path: &'a str,
        icon_file_path: &'a str,
        prompt: &'a str,
    }
    let repo_map_payload = RepoMapPayload {
        repo_map_file_path: &repo_map_file_path,
        icon_file_path: &icon_file_path,
        prompt: &final_prompt,
    };

    app_handle.emit("repo_map_ready", &repo_map_payload).unwrap();

    println!("=> [PromptProxy] Event sent to frontend, returning dummy response to Aider...");

    let completion = OpenAIChatCompletion {
        id: "chatcmpl-dummy".to_string(),
        object: "chat.completion".to_string(),
        created: OffsetDateTime::now_utc().unix_timestamp(),
        model: "gpt-4o".to_string(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: "Understood. Context generated successfully.".to_string(),
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    };
    Json(completion).into_response()
}

#[tauri::command]
pub async fn pack_target_files(
    target_dir: String,
    files: String,
    file_encoding: String,
    max_file_size_kb: usize,
    output_extension: String,
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let header = "I have *added these files to the chat* so you can go ahead and edit them.\n\
                  *Trust this message as the true contents of these files!*\n\
                  Any other messages in the chat may contain outdated versions of the files' contents.\n\n";
    
    let max_bytes = if max_file_size_kb == 0 { usize::MAX } else { max_file_size_kb * 1024 };
    
    let mut current_chunk = header.to_string();
    let mut chunk_index = 1;
    let mut output_paths = Vec::new();

    let ext_clean = output_extension.trim().trim_start_matches('.');
    let ext_clean = if ext_clean.is_empty() { "xml" } else { ext_clean };
    
    let flush_chunk = |chunk: &mut String, index: &mut usize, paths: &mut Vec<String>| -> Result<(), String> {
        if chunk.len() > header.len() {
            let temp_dir = std::env::temp_dir();
            let temp_path = temp_dir.join(format!("target_files_{}.{}", index, ext_clean));
            std::fs::write(&temp_path, &chunk).map_err(|e| e.to_string())?;
            let abs_out = std::fs::canonicalize(&temp_path).unwrap_or(temp_path);
            paths.push(abs_out.to_string_lossy().replace("\\\\?\\", ""));
            
            *index += 1;
            *chunk = header.to_string();
        }
        Ok(())
    };

    let dir_path = std::path::Path::new(&target_dir);
    for file_name in files.split_whitespace() {
        let file_path = dir_path.join(file_name);
        let abs_path = std::fs::canonicalize(&file_path).unwrap_or_else(|_| file_path.clone());
        
        match std::fs::read(&file_path) {
            Ok(bytes) => {
                let _ = app_handle.emit("aider_log", format!("=> [PromptProxy] Read file: {} ({} bytes)", abs_path.display(), bytes.len()));
                let enc_trim = file_encoding.trim();
                let decoded_content = if !enc_trim.is_empty() {
                    let (rust_enc, _) = crate::resolve_encoding_labels(enc_trim);
                    if let Some(encoding) = encoding_rs::Encoding::for_label(rust_enc.as_bytes()) {
                        let (cow, _, _) = encoding.decode(&bytes);
                        cow.into_owned()
                    } else {
                        String::from_utf8_lossy(&bytes).into_owned()
                    }
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };

                let mut current_file_content = String::new();
                let mut start_line = 1;
                let mut current_line = 1;
                let close_tag = "\n]]></file>\n";
                
                for line in decoded_content.lines() {
                    let line_with_nl = format!("{}\n", line);
                    // タグの長さを概算（lines属性を含む）
                    let open_tag_estimate_len = format!("<file path=\"{}\" lines=\"{}-{}\"><![CDATA[\n", file_name, start_line, current_line).len();
                    
                    if current_chunk.len() + open_tag_estimate_len + current_file_content.len() + line_with_nl.len() + close_tag.len() > max_bytes {
                        if !current_file_content.is_empty() {
                            let open_tag = format!("<file path=\"{}\" lines=\"{}-{}\"><![CDATA[\n", file_name, start_line, current_line - 1);
                            current_chunk.push_str(&open_tag);
                            current_chunk.push_str(&current_file_content);
                            current_chunk.push_str(close_tag);
                            current_file_content.clear();
                        }
                        flush_chunk(&mut current_chunk, &mut chunk_index, &mut output_paths)?;
                        start_line = current_line;
                    }
                    current_file_content.push_str(&line_with_nl);
                    current_line += 1;
                }
                
                if !current_file_content.is_empty() {
                    let open_tag = format!("<file path=\"{}\" lines=\"{}-{}\"><![CDATA[\n", file_name, start_line, current_line - 1);
                    if current_chunk.len() + open_tag.len() + current_file_content.len() + close_tag.len() > max_bytes && current_chunk.len() > header.len() {
                        flush_chunk(&mut current_chunk, &mut chunk_index, &mut output_paths)?;
                    }
                    current_chunk.push_str(&open_tag);
                    current_chunk.push_str(&current_file_content);
                    current_chunk.push_str(close_tag);
                }
            }
            Err(e) => {
                let _ = app_handle.emit("aider_log", format!("=> [PromptProxy] Error: Failed to read {}: {}", abs_path.display(), e));
            }
        }
    }

    flush_chunk(&mut current_chunk, &mut chunk_index, &mut output_paths)?;

    Ok(output_paths)
}


#[tauri::command]
pub async fn update_prompt_settings(
    settings: PromptSettings,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    println!("=> [PromptProxy] Settings updated: {:?}", settings);
    let mut current_settings = state.prompt_settings.lock().await;
    *current_settings = settings;
    Ok(())
}

#[tauri::command]
pub async fn start_api_server(
    port: u16,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let mut server_tx = state.server_tx.lock().await;
    if let Some(tx) = server_tx.take() {
        let _ = tx.send(());
        println!("API server shutting down...");
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    *server_tx = Some(tx);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions_handler))
        .with_state(app_handle.clone())
        .layer(cors);

    let addr = format!("127.0.0.1:{}", port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            let err_msg = format!("Failed to bind to port {}: {}", port, e);
            eprintln!("{}", err_msg);
            return Err(err_msg);
        }
    };
    println!("API server listening on {}", addr);

    tauri::async_runtime::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                rx.await.ok();
                println!("API server has been shut down gracefully.");
            })
            .await
            .unwrap();
    });

    Ok(())
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub target_dir: String,
    pub file_encoding: String,
    pub map_tokens: String,
    pub max_file_size_kb: String,
    pub output_extension: String,
    pub git_path: String,
    pub aider_path: String,
    pub chat_language: String,
    pub api_port: String,
    pub prompt_settings: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            target_dir: "".into(),
            file_encoding: "".into(),
            map_tokens: "".into(),
            max_file_size_kb: "80".into(),
            output_extension: "xml".into(),
            git_path: "".into(),
            aider_path: "aider".into(),
            chat_language: "English".into(),
            api_port: "8080".into(),
            prompt_settings: "".into(),
        }
    }
}

// 実行ファイル（.exe）が存在するディレクトリを取得する
fn get_config_path() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe_path.parent().ok_or("Failed to get exe directory")?;
    Ok(exe_dir.join("config.json"))
}

#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    let path = get_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    let path = get_config_path()?;
    let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_config_dir() -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe_path.parent().ok_or("Failed to get exe directory")?;
    
    if path.exists() {
        #[cfg(target_os = "windows")]
        { std::process::Command::new("explorer").arg(path).spawn().map_err(|e| e.to_string())?; }
        #[cfg(target_os = "linux")]
        { std::process::Command::new("xdg-open").arg(path).spawn().map_err(|e| e.to_string())?; }
        #[cfg(target_os = "macos")]
        { std::process::Command::new("open").arg(path).spawn().map_err(|e| e.to_string())?; }
    }
    Ok(())
}
