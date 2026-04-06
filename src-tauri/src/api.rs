use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;
use tokio::sync::{oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct PromptSettings {
    pub use_custom: bool,
    pub custom_edit_prompt: String,
    pub custom_ask_prompt: String,
}

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
    pub prompt_settings: Arc<Mutex<PromptSettings>>,
    pub server_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

async fn chat_completions_handler(
    State(app_handle): State<AppHandle>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    println!("=> [PromptProxy] Request received");
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
        "<instructions>\n\
        This file contains the source code of the target project and the overall repository context in XML format.\n\
        - The repository tag contains repository structure and important rules.\n\
        - The file path tag contains the actual code of each file.\n\
        Please modify the code referring to this context according to the user instructions provided separately.\n\
         </instructions>\n\n\
         <repository><![CDATA[{}]]></repository>\n{}",
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

    // AiderがSEARCH/REPLACEを要求しているか（Editモードか）を判定
    let expects_edit = context_content.contains("SEARCH/REPLACE");

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
     4. ONLY if you determine that necessary files are missing from context.xml, DO NOT output the code block. Instead, tell the user to add the missing file paths to the 'Target Files' input on the UI and try again."
        } else {
            "[IMPORTANT] Please output the answer to the user's question in natural text (code modification format is not required).\nIf you determine that necessary files are missing from context.xml to answer the question, please tell the user which files are missing and ask them to add those files to the 'Target Files' input and run again."
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

    println!("=> [PromptProxy] Event sent to frontend, waiting for response...");

    // Wait for the frontend to respond via the `respond_to_llm_request` command
    match rx.await {
        Ok(response_content) => {
            println!("=> [PromptProxy] Response received from frontend");
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
            println!("=> [PromptProxy] Returning response to Aider");
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
    println!("=> [PromptProxy] Text received from React: {}", response);
    if let Some(tx) = state.pending_requests.lock().await.remove(&request_id) {
        tx.send(response)
            .map_err(|_| "Failed to send response".to_string())
    } else {
        Err("Request ID not found".to_string())
    }
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
    state.pending_requests.lock().await.clear();
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
