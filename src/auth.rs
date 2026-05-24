/*
 * [125A-1+125A-2] Autenticación para gui_api: JWT, Argon2, rate limit, bootstrap admin.
 * LOCAL_MODE=true omite auth para el operador local sin interrumpir el flujo actual.
 * Gotcha: el rate limiter usa X-Real-IP (Traefik) como primaria; en dev local usa "unknown".
 * Pendiente: lista de tokens revocados para invalidación server-side (actualmente JWT stateless).
 */

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::{Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::RwLock;

use crate::error::CoolifyError;

const JWT_EXPIRY_SECS: u64 = 900; // 15 min
const RATE_LIMIT_MAX: u32 = 5;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(900); // 15 min

// ============================================================
// Tipos públicos
// ============================================================

#[derive(Clone)]
pub struct User {
    pub email: String,
    pub password_hash: String,
}

#[derive(Clone)]
pub struct AuthState {
    pub users: Arc<RwLock<Vec<User>>>,
    pub jwt_secret: Arc<String>,
    pub local_mode: bool,
    pub rate_limit: Arc<RwLock<HashMap<String, RateLimitEntry>>>,
}

#[derive(Clone)]
pub struct RateLimitEntry {
    pub intentos: u32,
    pub ventana_inicio: Instant,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
}

// ============================================================
// Request / response DTOs
// ============================================================

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub email: String,
    pub local_mode: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthErrorResponse {
    pub error: String,
}

// ============================================================
// AuthState impl
// ============================================================

impl AuthState {
    pub fn new(jwt_secret: String, local_mode: bool) -> Self {
        Self {
            users: Arc::new(RwLock::new(Vec::new())),
            jwt_secret: Arc::new(jwt_secret),
            local_mode,
            rate_limit: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn bootstrap_admin(&self, email: &str, password: &str) -> Result<(), CoolifyError> {
        let mut users = self.users.write().await;
        if users.iter().any(|u| u.email == email) {
            return Ok(());
        }
        let hash = hash_password(password)?;
        users.push(User {
            email: email.to_string(),
            password_hash: hash,
        });
        tracing::info!("Admin bootstrapped: {}", email);
        Ok(())
    }

    pub async fn has_users(&self) -> bool {
        !self.users.read().await.is_empty()
    }
}

// ============================================================
// Crypto helpers
// ============================================================

pub fn hash_password(password: &str) -> Result<String, CoolifyError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| CoolifyError::Validation(format!("Error hasheando contraseña: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

pub fn create_jwt(email: &str, secret: &str) -> Result<String, CoolifyError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = Claims {
        sub: email.to_string(),
        iat: now,
        exp: now + JWT_EXPIRY_SECS,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| CoolifyError::Validation(format!("Error creando JWT: {e}")))
}

pub fn validate_jwt(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|d| d.claims)
}

// ============================================================
// Rate limiter (in-memory, por IP)
// ============================================================

async fn check_rate_limit(
    rate_limit: &Arc<RwLock<HashMap<String, RateLimitEntry>>>,
    ip: &str,
) -> bool {
    let mut map = rate_limit.write().await;
    let now = Instant::now();
    if let Some(entry) = map.get_mut(ip) {
        if now.duration_since(entry.ventana_inicio) > RATE_LIMIT_WINDOW {
            entry.intentos = 1;
            entry.ventana_inicio = now;
            true
        } else if entry.intentos >= RATE_LIMIT_MAX {
            false
        } else {
            entry.intentos += 1;
            true
        }
    } else {
        map.insert(
            ip.to_string(),
            RateLimitEntry {
                intentos: 1,
                ventana_inicio: now,
            },
        );
        true
    }
}

fn extract_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-real-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

// ============================================================
// Handlers
// ============================================================

pub async fn login_handler(
    State(auth): State<AuthState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let ip = extract_ip(&headers);

    if !check_rate_limit(&auth.rate_limit, &ip).await {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(AuthErrorResponse {
                error: "Demasiados intentos. Intenta de nuevo en 15 minutos.".to_string(),
            }),
        ));
    }

    let users = auth.users.read().await;
    let user = users.iter().find(|u| u.email == req.email);

    let (hash, email) = match user {
        Some(u) => (u.password_hash.clone(), u.email.clone()),
        None => {
            /* Timing-safe: verificar hash dummy para no revelar si el email existe */
            drop(users);
            let _ = verify_password(
                &req.password,
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$RdescudvJCsgt3ub+b+dWRWJTmaaJObG",
            );
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: "Credenciales incorrectas".to_string(),
                }),
            ));
        }
    };
    drop(users);

    if !verify_password(&req.password, &hash) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Credenciales incorrectas".to_string(),
            }),
        ));
    }

    let token = create_jwt(&email, &auth.jwt_secret).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    tracing::info!("Login exitoso: {} desde {}", email, ip);
    Ok(Json(LoginResponse { token, email }))
}

pub async fn logout_handler() -> Json<Value> {
    /* MVP stateless — el logout limpia el token en el frontend.
     * Pendiente: lista de revocación server-side para invalidación inmediata. */
    Json(serde_json::json!({ "ok": true }))
}

pub async fn me_handler(
    State(auth): State<AuthState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<MeResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    if auth.local_mode {
        return Ok(Json(MeResponse {
            email: "local@operator".to_string(),
            local_mode: true,
        }));
    }

    let token = extract_bearer(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Sin token de autenticación".to_string(),
            }),
        )
    })?;

    let claims = validate_jwt(token, &auth.jwt_secret).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Sesión expirada o token inválido".to_string(),
            }),
        )
    })?;

    Ok(Json(MeResponse {
        email: claims.sub,
        local_mode: false,
    }))
}

// ============================================================
// Middleware
// ============================================================

/// Valida JWT en `Authorization: Bearer` para rutas protegidas.
/// Si `local_mode = true`, pasa todo sin verificar.
pub async fn auth_middleware(
    State(auth): State<AuthState>,
    request: Request,
    next: Next,
) -> Response {
    if auth.local_mode {
        return next.run(request).await;
    }

    let token = extract_bearer(request.headers());
    let Some(token) = token else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Autenticación requerida".to_string(),
            }),
        )
            .into_response();
    };

    if validate_jwt(token, &auth.jwt_secret).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(AuthErrorResponse {
                error: "Token inválido o expirado".to_string(),
            }),
        )
            .into_response();
    }

    next.run(request).await
}
