use crate::protocol::{
    AddDownloadHttpPayload, BatchWildcardRequest, FullDaemonState, RefreshLinkHttpPayload,
    SpeedLimitHttpPayload,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use oxide_core::{
    expand_wildcard_url, DownloadJobSnapshot, DownloadManager, DownloadRequest, QueueConfig,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

#[derive(Clone)]
pub struct HttpRpcState {
    pub manager: DownloadManager,
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    pub delete_file: Option<bool>,
}

/// Starts the local JSON + WebSocket RPC bridge on `127.0.0.1:46890` for browser extension & IPC integration.
pub async fn start_http_rpc_server(manager: DownloadManager, port: u16) {
    let state = HttpRpcState { manager };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/state", get(get_full_state))
        .route("/api/downloads", get(list_downloads).post(add_download))
        .route("/api/downloads/batch", post(add_batch_wildcard))
        .route("/api/downloads/pause-all", post(pause_all_downloads))
        .route("/api/downloads/resume-all", post(resume_all_downloads))
        .route("/api/downloads/:id/pause", post(pause_download))
        .route("/api/downloads/:id/resume", post(resume_download))
        .route("/api/downloads/:id/refresh", post(refresh_download))
        .route(
            "/api/downloads/:id/speed-limit",
            post(set_task_speed_limit),
        )
        .route("/api/downloads/:id", delete(delete_download))
        .route("/api/queues", get(list_queues).post(upsert_queue))
        .route("/api/speed-limit", post(set_global_speed_limit))
        .route("/ws", get(ws_handler))
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            info!("Oxide JSON/WS RPC Server listening on http://{}", addr);
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP RPC server error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to bind HTTP RPC server on {}: {}", addr, e);
        }
    }
}

fn build_full_state(manager: &DownloadManager) -> FullDaemonState {
    let downloads = manager.list_snapshots();
    let queues = manager.db().list_queues().unwrap_or_default();
    let global_speed_limit_bps = manager.get_global_speed_limit();
    FullDaemonState {
        downloads,
        queues,
        global_speed_limit_bps,
    }
}

async fn get_full_state(State(state): State<HttpRpcState>) -> Json<FullDaemonState> {
    Json(build_full_state(&state.manager))
}

async fn list_downloads(State(state): State<HttpRpcState>) -> Json<Vec<DownloadJobSnapshot>> {
    Json(state.manager.list_snapshots())
}

async fn add_download(
    State(state): State<HttpRpcState>,
    Json(payload): Json<AddDownloadHttpPayload>,
) -> impl IntoResponse {
    let (req, start_immediately) = payload.into_download_request();
    match state.manager.add_download(req, start_immediately).await {
        Ok(id) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "id": id })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn add_batch_wildcard(
    State(state): State<HttpRpcState>,
    Json(batch): Json<BatchWildcardRequest>,
) -> impl IntoResponse {
    let urls = expand_wildcard_url(&batch.pattern, batch.wildcard_count);
    let start_now = batch.start_immediately.unwrap_or(true);
    let mgr = state.manager.clone();

    tokio::spawn(async move {
        for url in urls {
            let req = DownloadRequest {
                url,
                filename: None,
                save_dir: batch.save_dir.clone(),
                headers: HashMap::new(),
                max_segments: batch.max_segments,
                speed_limit_bps: None,
                queue_id: batch.queue_id.clone(),
                expected_sha256: None,
            };
            let _ = mgr.add_download(req, start_now).await;
        }
    });

    (
        StatusCode::OK,
        Json(json!({ "status": "ok", "message": "Batch expansion queued" })),
    )
}

async fn pause_download(
    State(state): State<HttpRpcState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.manager.pause_download(&id).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn resume_download(
    State(state): State<HttpRpcState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.manager.resume_download(&id).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn refresh_download(
    State(state): State<HttpRpcState>,
    Path(id): Path<String>,
    Json(payload): Json<RefreshLinkHttpPayload>,
) -> impl IntoResponse {
    let (core_payload, auto_resume) = payload.into_core_payload();
    match state
        .manager
        .refresh_download_link(&id, core_payload, auto_resume)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn set_task_speed_limit(
    State(state): State<HttpRpcState>,
    Path(id): Path<String>,
    Json(payload): Json<SpeedLimitHttpPayload>,
) -> impl IntoResponse {
    match state.manager.set_task_speed_limit(&id, payload.bps) {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn delete_download(
    State(state): State<HttpRpcState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> impl IntoResponse {
    let delete_file = query.delete_file.unwrap_or(false);
    match state.manager.delete_download(&id, delete_file).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn pause_all_downloads(State(state): State<HttpRpcState>) -> impl IntoResponse {
    let _ = state.manager.pause_all().await;
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn resume_all_downloads(State(state): State<HttpRpcState>) -> impl IntoResponse {
    let _ = state.manager.resume_all().await;
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn list_queues(State(state): State<HttpRpcState>) -> Json<Vec<QueueConfig>> {
    Json(state.manager.db().list_queues().unwrap_or_default())
}

async fn upsert_queue(
    State(state): State<HttpRpcState>,
    Json(queue): Json<QueueConfig>,
) -> impl IntoResponse {
    match state.manager.db().upsert_queue(&queue) {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "status": "error", "message": e.to_string() })),
        ),
    }
}

async fn set_global_speed_limit(
    State(state): State<HttpRpcState>,
    Json(payload): Json<SpeedLimitHttpPayload>,
) -> impl IntoResponse {
    state.manager.set_global_speed_limit(payload.bps);
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<HttpRpcState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_client(socket, state.manager))
}

async fn handle_ws_client(mut socket: WebSocket, manager: DownloadManager) {
    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    loop {
        ticker.tick().await;
        let payload = build_full_state(&manager);
        let Ok(json_text) = serde_json::to_string(&payload) else {
            continue;
        };
        if socket.send(Message::Text(json_text)).await.is_err() {
            break;
        }
    }
}
