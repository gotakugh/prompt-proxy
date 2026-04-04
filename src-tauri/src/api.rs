use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;
use tokio::sync::{oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

type ResponderTx = oneshot::Sender<String>;

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
    pub pending_requests: Arc<Mutex<HashMap<String, ResponderTx>>>,
}

async fn chat_completions_handler(
    State(app_handle): State<AppHandle>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    println!("=> [PromptProxy] リクエストを受信しました");
    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<String>();

    // Access the state managed by Tauri
    let state: tauri::State<AppState> = app_handle.state();
    state
        .pending_requests
        .lock()
        .await
        .insert(request_id.clone(), tx);

    // 1. Parse OpenAI JSON and separate context from user instruction
    let messages = match payload.get("messages").and_then(|m| m.as_array()) {
        Some(m) => m,
        None => {
            return Json(json!({"error": "messages field is missing or not an array"}))
                .into_response()
        }
    };

    let last_user_message_index = match messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
    {
        Some(i) => i,
        None => return Json(json!({"error": "No user message found in messages"})).into_response(),
    };

    let user_instruction = messages[last_user_message_index]
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    let context_content: String = messages[..last_user_message_index]
        .iter()
        .filter_map(|msg| msg.get("content").and_then(|c| c.as_str()))
        .collect::<Vec<_>>()
        .join("\n");

    // 2. Extract context and format as Repomix-style XML
    let mut repo_info = String::new();
    let mut files_xml = String::new();
    let mut lines = context_content.lines().peekable();

    while let Some(line) = lines.next() {
        let is_potential_path = !line.trim().is_empty() && !line.starts_with(' ');
        if is_potential_path {
            if let Some(next_line) = lines.peek() {
                if next_line.starts_with("```") {
                    let path = line.trim();
                    lines.next(); // Consume ```
                    let mut code = String::new();
                    for code_line in lines.by_ref() {
                        if code_line.starts_with("```") {
                            break;
                        }
                        code.push_str(code_line);
                        code.push('\n');
                    }
                    files_xml.push_str(&format!(
                        "<file path=\"{}\"><![CDATA[{}]]></file>\n",
                        path, code
                    ));
                    continue;
                }
            }
        }
        repo_info.push_str(line);
        repo_info.push('\n');
    }

    let xml_string = format!(
        "<repository><![CDATA[{}]]></repository>\n{}",
        repo_info, files_xml
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

    let temp_path = temp_dir.join("context.xml");

    if let Err(e) = fs::write(&temp_path, xml_string) {
        return Json(json!({ "error": format!("Failed to write context.xml: {}", e)}))
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

    let mut context_file_path = match absolute_path.to_str() {
        Some(s) => s.to_string(),
        None => return Json(json!({"error": "Temp path contains invalid UTF-8"})).into_response(),
    };
    // Remove the UNC path prefix on Windows to ensure the OS drag API works correctly.
    if context_file_path.starts_with("\\\\?\\") {
        context_file_path = context_file_path.replace("\\\\?\\", "");
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

    let absolute_json_path = match std::fs::canonicalize(&json_path) {
        Ok(path) => path,
        Err(e) => {
            return Json(
                json!({ "error": format!("Failed to get absolute path for aider_payload.json: {}", e)}),
            )
            .into_response()
        }
    };
    let mut json_file_path = match absolute_json_path.to_str() {
        Some(s) => s.to_string(),
        None => return Json(json!({"error": "JSON path contains invalid UTF-8"})).into_response(),
    };
    if json_file_path.starts_with("\\\\?\\") {
        json_file_path = json_file_path.replace("\\\\?\\", "");
    }

    // 3. Create final prompt
    let final_prompt = format!(
        "{}\n\n【重要】出力は挨拶や解説を一切省き、SEARCH/REPLACEブロックのみを使用すること。",
        user_instruction
    );

    println!("=> [PromptProxy] XMLとプロンプトの生成が完了しました");

    // 4. Emit data to frontend
    #[derive(Clone, serde::Serialize)]
    struct PromptPayload<'a> {
        request_id: &'a str,
        context_file_path: &'a str,
        icon_file_path: &'a str,
        json_file_path: &'a str,
        prompt: &'a str,
    }
    let prompt_payload = PromptPayload {
        request_id: &request_id,
        context_file_path: &context_file_path,
        icon_file_path: &icon_file_path,
        json_file_path: &json_file_path,
        prompt: &final_prompt,
    };

    app_handle.emit("prompt_received", &prompt_payload).unwrap();

    println!("=> [PromptProxy] フロントエンドにイベントを送信し、待機を開始します...");

    // Wait for the frontend to respond via the `respond_to_llm_request` command
    match rx.await {
        Ok(response_content) => {
            println!("=> [PromptProxy] フロントエンドから返答を受け取りました");
            let completion = OpenAIChatCompletion {
                id: "chatcmpl-dummy".to_string(),
                object: "chat.completion".to_string(),
                created: OffsetDateTime::now_utc().unix_timestamp(),
                model: "gpt-4o".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Message {
                        role: "assistant".to_string(),
                        content: response_content,
                    },
                    finish_reason: "stop".to_string(),
                }],
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
            println!("=> [PromptProxy] Aiderへレスポンスを返却します");
            Json(completion).into_response()
        }
        Err(_) => {
            println!(
                "=> [PromptProxy] Error: oneshot channel was closed before receiving a response."
            );
            Json(json!({ "error": "Internal error: oneshot channel was closed" })).into_response()
        }
    }
}

// This is the command that the frontend will call to provide the response.
#[tauri::command]
pub async fn respond_to_llm_request(
    request_id: String,
    response: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    println!("=> [PromptProxy] Reactから受信したテキスト: {}", response);
    if let Some(tx) = state.pending_requests.lock().await.remove(&request_id) {
        tx.send(response)
            .map_err(|_| "Failed to send response".to_string())
    } else {
        Err("Request ID not found".to_string())
    }
}

// This function initializes and runs the Axum server in a background task.
pub fn init(app_handle: &AppHandle) {
    // Create and manage our application state
    let state = AppState {
        pending_requests: Arc::new(Mutex::new(HashMap::new())),
    };
    app_handle.manage(state);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions_handler))
        .with_state(app_handle.clone())
        .layer(cors);

    let _server_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
            .await
            .unwrap();
        println!("API server listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, app).await.unwrap();
    });
}
