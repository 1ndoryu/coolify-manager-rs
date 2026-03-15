/*
 * Comando: auth-drive
 * Flujo OAuth para autorizar Google Drive con cuenta personal.
 * Obtiene un refresh_token que permite subir backups usando la cuota del usuario.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::google_drive::GoogleDriveClient;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;

pub async fn execute(config_path: &Path) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    let drive_config = match &settings.backup_storage.remote {
        Some(crate::config::RemoteBackupConfig::GoogleDrive(config)) => config,
        None => {
            return Err(CoolifyError::Validation(
                "No hay configuracion remota de Google Drive en settings.json".to_string(),
            ))
        }
    };

    let client_id = drive_config.oauth_client_id.as_deref().ok_or_else(|| {
        CoolifyError::Validation("Falta oauthClientId en la configuracion de Google Drive".to_string())
    })?;
    let client_secret = drive_config.oauth_client_secret.as_deref().ok_or_else(|| {
        CoolifyError::Validation("Falta oauthClientSecret en la configuracion de Google Drive".to_string())
    })?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| CoolifyError::Validation(format!("No se pudo abrir puerto local: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| CoolifyError::Validation(format!("No se pudo leer puerto: {error}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let auth_url = GoogleDriveClient::build_oauth_url(client_id, &redirect_uri);

    println!("Abre esta URL en tu navegador para autorizar Google Drive:\n");
    println!("{auth_url}\n");
    println!("Esperando autorizacion en {redirect_uri} ...");

    let code = wait_for_auth_code(&listener)?;
    println!("Codigo recibido, intercambiando por tokens...");

    let (_access_token, refresh_token) =
        GoogleDriveClient::exchange_auth_code(client_id, client_secret, &code, &redirect_uri).await?;

    println!("\nAutorizacion exitosa.");
    println!("Agrega esta variable a tu .env:\n");
    println!("GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN={refresh_token}");
    println!("\nY en settings.json agrega dentro de backupStorage.remote:");
    println!("  \"oauthRefreshToken\": \"${{GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN}}\"");

    /* Intentar escribir automaticamente en .env */
    let env_path = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".env");
    if env_path.exists() {
        let content = std::fs::read_to_string(&env_path).unwrap_or_default();
        if content.contains("GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN") {
            let updated = content
                .lines()
                .map(|line| {
                    if line.starts_with("GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN") {
                        format!("GOOGLE_DRIVE_OAUTH_REFRESH_TOKEN={refresh_token}")
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&env_path, updated)?;
            println!("\n.env actualizado automaticamente.");
        } else {
            let mut file = std::fs::OpenOptions::new().append(true).open(&env_path)?;
            writeln!(file, "\nGOOGLE_DRIVE_OAUTH_REFRESH_TOKEN={refresh_token}")?;
            println!("\nRefresh token agregado al .env automaticamente.");
        }
    }

    Ok(())
}

fn wait_for_auth_code(listener: &TcpListener) -> std::result::Result<String, CoolifyError> {
    let (mut stream, _) = listener
        .accept()
        .map_err(|error| CoolifyError::Validation(format!("Error aceptando conexion: {error}")))?;

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| CoolifyError::Validation(format!("Error leyendo request: {error}")))?;

    let code = extract_code_from_request(&request_line)?;

    let response_body = "Autorizacion completada. Puedes cerrar esta ventana.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response_body.len(),
        response_body,
    );
    let _ = stream.write_all(response.as_bytes());

    Ok(code)
}

fn extract_code_from_request(request_line: &str) -> std::result::Result<String, CoolifyError> {
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| CoolifyError::Validation("Request HTTP invalido".to_string()))?;

    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .ok_or_else(|| CoolifyError::Validation("No se encontraron parametros en la URL".to_string()))?;

    for param in query.split('&') {
        if let Some(("code", value)) = param.split_once('=') {
            return Ok(value.to_string());
        }
    }

    if query.contains("error=") {
        return Err(CoolifyError::Validation(format!(
            "Google devolvio un error: {query}"
        )));
    }

    Err(CoolifyError::Validation("No se encontro el parametro 'code' en el callback".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code() {
        let request = "GET /?code=4/0AcvDMrK2k3example&scope=https://www.googleapis.com/auth/drive HTTP/1.1";
        let code = extract_code_from_request(request).unwrap();
        assert_eq!(code, "4/0AcvDMrK2k3example");
    }

    #[test]
    fn test_extract_code_error() {
        let request = "GET /?error=access_denied HTTP/1.1";
        let result = extract_code_from_request(request);
        assert!(result.is_err());
    }
}
