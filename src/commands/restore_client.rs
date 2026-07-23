/*
 * Comando: restore-client
 * Restaura datos de un cliente vía el endpoint de bootstrap de la API.
 * Ejecuta login + bootstrap + vinculación de Stripe dentro del contenedor app.
 *
 * Flujo: curl login → obtener JWT → curl bootstrap → (opcional) SQL para Stripe.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::docker;
use crate::infra::pg_utils;
use crate::infra::ssh_client::SshClient;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
    admin_email: &str,
    admin_password: &str,
    stripe_sub_id: Option<&str>,
    dry_run: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target_config = settings.resolve_site_target(site)?;

    let mut ssh = SshClient::from_vps(&target_config.vps);
    ssh.connect().await?;

    let app_container = docker::find_app_container(&ssh, stack_uuid).await?;
    let (pg_container, db_user, db_name, _) =
        pg_utils::get_pg_credentials(&ssh, stack_uuid).await?;

    println!("[restore-client] Restaurando datos del cliente...");

    if dry_run {
        println!("[restore-client] dry-run: solo verificando estado, no se ejecutará el bootstrap.");
        let status_sql = "SELECT count(*)::text FROM hosting_subscriptions WHERE client_email = 'guillermo@nakomi.com';";
        let count = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, status_sql).await?;
        println!(
            "[restore-client] Hostings existentes para guillermo@nakomi.com: {}",
            count.trim()
        );
        return Ok(());
    }

    /* Paso 1: Login como admin para obtener JWT */
    let admin_email_escaped = admin_email.replace('"', "\\\"");
    let admin_password_escaped = admin_password.replace('"', "\\\"");
    let login_cmd = format!(
        "curl -s -X POST http://localhost:3000/api/auth/login -H 'Content-Type: application/json' -d '{{\"email\":\"{}\",\"password\":\"{}\"}}'",
        admin_email_escaped, admin_password_escaped
    );

    tracing::info!("Autenticando como admin: {}", admin_email);
    let login_result = docker::docker_exec(&ssh, &app_container, &login_cmd).await?;

    if !login_result.success() {
        return Err(CoolifyError::Validation(format!(
            "Login falló: {}",
            login_result.stderr
        )));
    }

    let token = extract_token(&login_result.stdout)?;
    println!("  ✅ Login exitoso como {}", admin_email);

    /* Paso 2: Ejecutar bootstrap de Guillermo */
    let bootstrap_cmd = format!(
        "curl -s -X POST http://localhost:3000/api/admin/client-bootstrap/guillermo -H 'Content-Type: application/json' -H 'Authorization: Bearer {}' -d '{{\"temporary_password\":\"Guillermo2026!\"}}'",
        token
    );

    tracing::info!("Ejecutando bootstrap de Guillermo...");
    let bootstrap_result = docker::docker_exec(&ssh, &app_container, &bootstrap_cmd).await?;

    if !bootstrap_result.success() {
        return Err(CoolifyError::Validation(format!(
            "Bootstrap falló: {}",
            bootstrap_result.stderr
        )));
    }

    println!("  ✅ Bootstrap ejecutado:");
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&bootstrap_result.stdout) {
        if let Some(uid) = json.get("user_id").and_then(|v| v.as_str()) {
            println!("     user_id:              {}", uid);
        }
        if let Some(created) = json.get("user_created").and_then(|v| v.as_bool()) {
            println!("     user_created:         {}", created);
        }
        if let Some(h) = json.get("hostings_upserted").and_then(|v| v.as_u64()) {
            println!("     hostings_upserted:    {}", h);
        }
        if let Some(b) = json.get("billing_items_upserted").and_then(|v| v.as_u64()) {
            println!("     billing_items_upserted: {}", b);
        }
        if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
            println!("     message:              {}", msg);
        }
    } else {
        println!("     {}", bootstrap_result.stdout.trim());
    }

    /* Paso 3: Vincular Stripe subscription si se proporcionó */
    if let Some(sub_id) = stripe_sub_id {
        println!();
        println!("  🔗 Vinculando Stripe subscription {}...", sub_id);

        /* Escapar el sub_id para prevenir SQL injection */
        let sub_id_safe = sub_id.replace('\'', "''");
        let update_sql = format!(
            "UPDATE hosting_subscriptions SET stripe_subscription_id = '{}', updated_at = NOW() WHERE domain = 'cap.wandori.us' AND (stripe_subscription_id IS NULL OR stripe_subscription_id = '');",
            sub_id_safe
        );
        let affected = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, &update_sql).await?;
        let affected = affected.trim();
        if affected.starts_with("UPDATE") {
            println!("     ✅ {}", affected);
        } else {
            println!("     Resultado: {}", affected);
        }

        /* Verificar que quedó vinculado */
        let verify_sql = "SELECT stripe_subscription_id FROM hosting_subscriptions WHERE domain = 'cap.wandori.us';";
        let current_id = pg_utils::run_pg_query(&ssh, &pg_container, &db_user, &db_name, verify_sql).await?;
        let current_id = current_id.trim();
        if current_id == sub_id {
            println!("     ✅ Verificado: cap.wandori.us → {}", current_id);
        } else {
            println!("     ⚠️  Valor actual: '{}' (esperado: '{}')", current_id, sub_id);
        }
    }

    println!();
    println!("[restore-client] Restauración completada.");

    Ok(())
}

fn extract_token(json_str: &str) -> std::result::Result<String, CoolifyError> {
    let parsed: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        CoolifyError::Validation(format!("Error parseando respuesta de login: {}", e))
    })?;

    parsed
        .get("token")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| {
            CoolifyError::Validation("Campo 'token' no encontrado en respuesta de login".into())
        })
}
