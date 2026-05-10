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
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct GuiApiState {
    config_path: Arc<PathBuf>,
    cache: Arc<RwLock<HashMap<String, CachedValue>>>,
}

#[derive(Clone)]
struct CachedValue {
    created_at: Instant,
    value: Value,
}

/* [105A-28] Cache TTL en el limite HTTP local: las lecturas caras se reutilizan entre vistas
 * y los botones de refresco pasan force=true para pedir datos frescos. */

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
        cache: Arc::new(RwLock::new(HashMap::new())),
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
    match execute_command(&state, request).await {
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
    state: &GuiApiState,
    request: GuiCommandRequest,
) -> Result<Value, CoolifyError> {
    let config_path = state.config_path.as_path();
    let args = request.args;
    let command = request.command;
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    let cache_ttl = command_cache_ttl(&command);
    let cache_key = command_cache_key(&command, &args);

    if let Some(ttl) = cache_ttl.filter(|_| !force) {
        if let Some(cached) = state.cache.read().await.get(&cache_key) {
            if cached.created_at.elapsed() <= ttl {
                return Ok(cached.value.clone());
            }
        }
    }

    let value = match command.as_str() {
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
    }?;

    if cache_ttl.is_some() {
        state.cache.write().await.insert(
            cache_key,
            CachedValue {
                created_at: Instant::now(),
                value: value.clone(),
            },
        );
    }

    Ok(value)
}

fn command_cache_ttl(command: &str) -> Option<Duration> {
    match command {
        "list_sites" | "list_targets" => Some(Duration::from_secs(60)),
        "health_check" | "audit_vps" => Some(Duration::from_secs(20)),
        "deployment_metrics" => Some(Duration::from_secs(12)),
        "list_backups" => Some(Duration::from_secs(180)),
        "list_all_backups" => Some(Duration::from_secs(300)),
        "get_config_path" => Some(Duration::from_secs(300)),
        _ => None,
    }
}

fn command_cache_key(command: &str, args: &Value) -> String {
    let mut normalized_args = args.clone();
    if let Some(object) = normalized_args.as_object_mut() {
        object.remove("force");
    }

    format!("{command}:{normalized_args}")
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
