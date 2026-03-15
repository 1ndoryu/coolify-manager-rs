/*
 * Comando: deploy-websocket
 * Agrega el servicio WebSocket a un stack existente de Kamples.
 * Lee el docker-compose actual del stack, inyecta el servicio websocket,
 * y actualiza via Coolify API.
 *
 * Uso: coolify-manager deploy-websocket --name kamples
 *
 * Flujo:
 * 1. Lee docker-compose actual del stack via Coolify API
 * 2. Inyecta servicio websocket + env vars WS en wordpress
 * 3. Actualiza docker-compose en Coolify via API PATCH
 * 4. Reinicia el stack para crear el contenedor websocket
 */

use crate::config::Settings;
use crate::domain::StackTemplate;
use crate::error::CoolifyError;
use crate::infra::coolify_api::CoolifyApiClient;
use crate::infra::template_engine;
use crate::infra::validation;

use std::path::Path;

pub async fn execute(
    config_path: &Path,
    site_name: &str,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let site = settings.get_site(site_name)?;
    validation::assert_site_ready(site)?;

    if site.template != StackTemplate::Kamples {
        return Err(CoolifyError::Validation(format!(
            "El sitio '{site_name}' no es un stack Kamples (template: {}). WebSocket solo aplica a Kamples.",
            site.template
        )));
    }

    let stack_uuid = site.stack_uuid.as_deref().unwrap();
    let target = settings.resolve_site_target(site)?;
    let api = CoolifyApiClient::new(&target.coolify)?;

    tracing::info!("Desplegando servicio WebSocket para '{site_name}' (stack: {stack_uuid})");

    /* Leer compose actual del stack */
    let service_info = api.get_service(stack_uuid).await?;
    let current_compose = service_info
        .get("docker_compose_raw")
        .or_else(|| service_info.get("docker_compose"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if current_compose.is_empty() {
        return Err(CoolifyError::Validation(
            "No se pudo obtener el docker-compose actual del stack. Verificar en Coolify dashboard.".into()
        ));
    }

    tracing::info!("Docker-compose actual obtenido ({} bytes)", current_compose.len());

    /* Verificar que no tiene ya un servicio websocket */
    if current_compose.contains("websocket:") || current_compose.contains("SERVICE_FQDN_WEBSOCKET") {
        tracing::warn!("El stack ya contiene un servicio websocket. Actualizando igualmente.");
    }

    /* Generar secrets WS */
    let ws_internal_secret = template_engine::generate_password(32);
    let ws_ticket_secret = template_engine::generate_password(32);

    let domain_clean = site.dominio
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let ws_domain = format!("https://ws.{domain_clean}");
    let ws_public_url = format!("wss://ws.{domain_clean}");

    /* Inyectar env vars WS en el servicio wordpress del compose existente */
    let mut compose = current_compose.clone();

    /* Agregar env vars WS al servicio wordpress (antes de DEV: "FALSE") */
    if !compose.contains("KAMPLES_WS_INTERNAL_SECRET") {
        compose = compose.replace(
            "DEV: \"FALSE\"",
            &format!(
                "KAMPLES_WS_INTERNAL_SECRET: {ws_internal_secret}\n            KAMPLES_WS_TICKET_SECRET: {ws_ticket_secret}\n            KAMPLES_WS_NOTIFY_URL: http://websocket:8080/notify\n            KAMPLES_WS_PUBLIC_URL: {ws_public_url}\n            DEV: \"FALSE\""
            )
        );
    }

    /* Inyectar servicio websocket antes de 'volumes:' */
    let ws_service = format!(
        r#"
    websocket:
        build:
            context: .
            dockerfile_inline: |
                FROM oven/bun:1
                WORKDIR /app
                RUN bun -e "const r=await fetch('https://raw.githubusercontent.com/1ndoryu/glorytemplate/{glory_branch}/websocket-server/server.ts');if(!r.ok)throw new Error('Fetch failed: '+r.status);await Bun.write('server.ts',await r.text())"
                EXPOSE 8080
                CMD ["bun", "run", "server.ts"]
        environment:
            KAMPLES_WS_INTERNAL_SECRET: {ws_internal_secret}
            KAMPLES_WS_TICKET_SECRET: {ws_ticket_secret}
            WS_PORT: "8080"
            SERVICE_FQDN_WEBSOCKET: {ws_domain}
        healthcheck:
            test: ["CMD", "bun", "-e", "fetch('http://localhost:8080/health').then(r=>r.ok?process.exit(0):process.exit(1)).catch(()=>process.exit(1))"]
            interval: 30s
            timeout: 5s
            retries: 3
        restart: unless-stopped
"#,
        glory_branch = site.glory_branch,
        ws_internal_secret = ws_internal_secret,
        ws_ticket_secret = ws_ticket_secret,
        ws_domain = ws_domain,
    );

    /* Insertar antes de la seccion volumes: */
    if let Some(pos) = compose.find("\nvolumes:") {
        compose.insert_str(pos as usize, &ws_service);
    } else {
        /* Si no hay seccion volumes, agregar al final */
        compose.push_str(&ws_service);
    }

    /* Actualizar docker-compose en Coolify */
    api.update_stack_compose(stack_uuid, &compose).await?;

    tracing::info!("Docker-compose actualizado con servicio WebSocket");

    /* Reiniciar stack para que se cree el contenedor websocket */
    tracing::info!("Reiniciando stack para crear contenedor WebSocket...");
    api.stop_service(stack_uuid).await?;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    api.start_service(stack_uuid).await?;

    println!("Servicio WebSocket desplegado para '{site_name}'.");
    println!("  WS Domain: {ws_public_url}");
    println!("  WS Internal: http://websocket:8080/notify");
    println!("  IMPORTANTE: Crear registro DNS A para ws.{domain_clean} apuntando al VPS ({}).", target.vps.ip);
    Ok(())
}
