/*
 * API HTTP local para la GUI web.
 * Permite usar Vite sin Tauri manteniendo datos reales desde coolify-manager.
 */

use crate::api;
use crate::error::CoolifyError;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct GuiApiState {
    config_path: Arc<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct GuiCommandRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Debug, Serialize)]
struct GuiErrorResponse {
    error: String,
}

pub async fn run(config_path: PathBuf, bind: SocketAddr) -> Result<(), CoolifyError> {
    let state = GuiApiState {
        config_path: Arc::new(config_path),
    };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/command", post(command))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!("GUI API local escuchando en http://{}", bind);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn command(
    State(state): State<GuiApiState>,
    Json(request): Json<GuiCommandRequest>,
) -> Result<Json<Value>, (StatusCode, Json<GuiErrorResponse>)> {
    match execute_command(&state.config_path, request).await {
        Ok(value) => Ok(Json(value)),
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(GuiErrorResponse {
                error: format!("{error:#}"),
            }),
        )),
    }
}

async fn execute_command(
    config_path: &Path,
    request: GuiCommandRequest,
) -> Result<Value, CoolifyError> {
    let args = request.args;
    match request.command.as_str() {
        "list_sites" => json_value(api::list_sites(config_path).await?),
        "list_targets" => json_value(api::list_targets(config_path).await?),
        "health_check" => {
            json_value(api::health_check(config_path, &arg_string(&args, "siteName")?).await?)
        }
        "list_backups" => {
            json_value(api::list_backups(config_path, &arg_string(&args, "siteName")?).await?)
        }
        "list_all_backups" => json_value(api::list_all_backups(config_path).await?),
        "audit_vps" => {
            json_value(api::audit_vps(config_path, opt_string(&args, "target").as_deref()).await?)
        }
        "deployment_metrics" => json_value(api::deployment_metrics(config_path).await?),
        "view_logs" => json_value(
            api::view_logs(
                config_path,
                &arg_string(&args, "siteName")?,
                opt_u32(&args, "lines").unwrap_or(120),
                opt_string(&args, "containerTarget").as_deref(),
            )
            .await?,
        ),
        "manual_backup" => {
            json_value(api::manual_backup(config_path, &arg_string(&args, "siteName")?).await?)
        }
        "restart_site" => {
            json_value(api::restart_site(config_path, &arg_string(&args, "siteName")?).await?)
        }
        "redeploy_site" => {
            json_value(api::redeploy_site(config_path, &arg_string(&args, "siteName")?).await?)
        }
        "get_config_path" => json_value(config_path.display().to_string()),
        other => Err(CoolifyError::Validation(format!(
            "Comando GUI no soportado: {other}"
        ))),
    }
}

fn json_value<T: Serialize>(value: T) -> Result<Value, CoolifyError> {
    serde_json::to_value(value).map_err(|error| {
        CoolifyError::Validation(format!("No se pudo serializar respuesta GUI: {error}"))
    })
}

fn arg_string(args: &Value, key: &str) -> Result<String, CoolifyError> {
    opt_string(args, key).ok_or_else(|| {
        CoolifyError::Validation(format!("Falta argumento requerido para GUI: {key}"))
    })
}

fn opt_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn opt_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}
