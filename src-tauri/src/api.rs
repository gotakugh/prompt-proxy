use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tauri::{AppHandle, Manager};
use tokio::sync::{oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

type ResponderTx = oneshot::Sender<String>;

// This state will be managed by Tauri and accessible from both
// the Axum handlers and Tauri commands.
pub struct AppState {
    pub pending_requests: Arc<Mutex<HashMap<String, ResponderTx>>>,
}

async fn chat_completions_handler(
    State(app_handle): State<AppHandle>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel::<String>();

    // Access the state managed by Tauri
    let state: tauri::State<AppState> = app_handle.state();
    state
        .pending_requests
        .lock()
        .await
        .insert(request_id.clone(), tx);

    // Emit an event to the frontend with the request payload and a unique ID
    app_handle
        .emit("new-llm-request", (&request_id, &payload))
        .unwrap();

    // Wait for the frontend to respond via the `respond_to_llm_request` command
    match rx.await {
        Ok(response_body) => match serde_json::from_str(&response_body) {
            Ok(json_value) => Json(json_value),
            Err(_) => Json(json!({"error": "Failed to parse JSON response from frontend"})),
        },
        Err(_) => Json(json!({ "error": "Internal error: oneshot channel was closed" })),
    }
}

// This is the command that the frontend will call to provide the response.
#[tauri::command]
pub async fn respond_to_llm_request(
    request_id: String,
    response: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
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
        println!(
            "API server listening on {}",
            listener.local_addr().unwrap()
        );
        axum::serve(listener, app).await.unwrap();
    });
}
