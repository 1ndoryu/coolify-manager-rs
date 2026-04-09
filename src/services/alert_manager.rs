/* [N2] Sistema de alertas por email.
 * Envia notificaciones SMTP cuando un sitio cae o tiene problemas criticos.
 * Usa la configuracion SMTP global de settings.json (Brevo/Sendinblue).
 *
 * Gotcha: lettre requiere feature "tokio1-rustls-tls" para async SMTP.
 * El from_email se toma del campo user de SMTP cuando no hay fromEmail explicito. */

use crate::config::{Settings, SmtpGlobalConfig};
use crate::error::CoolifyError;
use crate::services::health_manager::HealthReport;

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Envia un email de alerta generico.
pub async fn send_alert(
    smtp: &SmtpGlobalConfig,
    to_email: &str,
    subject: &str,
    body: &str,
) -> std::result::Result<(), CoolifyError> {
    let from = format!("{} <{}>", smtp.from_name, smtp.user);
    let email = Message::builder()
        .from(from.parse().map_err(|e| {
            CoolifyError::Validation(format!("Direccion from invalida '{}': {e}", smtp.user))
        })?)
        .to(to_email.parse().map_err(|e| {
            CoolifyError::Validation(format!("Direccion to invalida '{to_email}': {e}"))
        })?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| CoolifyError::Validation(format!("Error construyendo email: {e}")))?;

    let creds = Credentials::new(smtp.user.clone(), smtp.password.clone());

    let transport = match smtp.secure.as_str() {
        "ssl" => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
            .map_err(|e| CoolifyError::Validation(format!("Error SMTP relay: {e}")))?
            .port(smtp.port)
            .credentials(creds)
            .build(),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
            .map_err(|e| CoolifyError::Validation(format!("Error SMTP STARTTLS relay: {e}")))?
            .port(smtp.port)
            .credentials(creds)
            .build(),
    };

    transport.send(email).await.map_err(|e| {
        CoolifyError::Validation(format!("Error enviando email a {to_email}: {e}"))
    })?;

    tracing::info!("Alerta enviada a {to_email}: {subject}");
    Ok(())
}

/// Envia alerta especifica de sitio caido basada en un HealthReport.
pub async fn alert_site_down(
    settings: &Settings,
    report: &HealthReport,
) -> std::result::Result<(), CoolifyError> {
    let smtp = settings.smtp.as_ref().ok_or_else(|| {
        CoolifyError::Validation(
            "No hay configuracion SMTP en settings.json para enviar alertas".to_string(),
        )
    })?;

    let to_email = &settings.wordpress.default_admin_email;
    let subject = format!("ALERTA: Sitio {} caido", report.site_name);

    let mut body = format!(
        "El sitio {} ({}) reporta problemas:\n\n",
        report.site_name, report.url
    );
    body.push_str(&format!("HTTP OK: {}\n", report.http_ok));
    body.push_str(&format!("App OK: {}\n", report.app_ok));
    body.push_str(&format!(
        "Fatal log: {}\n",
        report.fatal_log_detected
    ));
    if let Some(status) = report.status_code {
        body.push_str(&format!("Status code: {status}\n"));
    }
    if !report.details.is_empty() {
        body.push_str("\nDetalles:\n");
        for detail in &report.details {
            body.push_str(&format!("- {detail}\n"));
        }
    }
    body.push_str(&format!(
        "\nTimestamp: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));

    send_alert(smtp, to_email, &subject, &body).await
}

/// Verifica la salud de todos los sitios y alerta sobre los que estan caidos.
/// Retorna la lista de sitios con problemas.
pub async fn check_and_alert_all_sites(
    settings: &Settings,
    _config_path: &std::path::Path,
) -> std::result::Result<Vec<HealthReport>, CoolifyError> {
    use crate::infra::ssh_client::SshClient;
    use crate::services::health_manager;

    let mut unhealthy_reports = Vec::new();

    for site in &settings.sitios {
        let target = settings.resolve_site_target(site)?;
        let mut ssh = SshClient::from_vps(&target.vps);
        if let Err(e) = ssh.connect().await {
            tracing::error!("No se pudo conectar a VPS para {}: {e}", site.nombre);
            continue;
        }

        match health_manager::run_site_health_check(settings, site, &ssh).await {
            Ok(report) => {
                if !report.healthy() {
                    println!(
                        "ALERTA: {} esta caido — enviando notificacion",
                        site.nombre
                    );
                    if let Err(e) = alert_site_down(settings, &report).await {
                        tracing::error!(
                            "No se pudo enviar alerta para {}: {e}",
                            site.nombre
                        );
                    }
                    unhealthy_reports.push(report);
                }
            }
            Err(e) => {
                tracing::error!("Error verificando {}: {e}", site.nombre);
            }
        }
    }

    if unhealthy_reports.is_empty() {
        println!("Todos los sitios saludables.");
    } else {
        println!(
            "{} sitio(s) con problemas — alertas enviadas.",
            unhealthy_reports.len()
        );
    }

    /* Si hay sitios caidos, enviar resumen consolidado */
    if unhealthy_reports.len() > 1 {
        let smtp = settings.smtp.as_ref();
        if let Some(smtp) = smtp {
            let subject = format!(
                "RESUMEN: {} sitios con problemas",
                unhealthy_reports.len()
            );
            let mut body = String::from("Sitios con problemas detectados:\n\n");
            for r in &unhealthy_reports {
                body.push_str(&format!("- {} ({})\n", r.site_name, r.url));
            }
            body.push_str(&format!(
                "\nTimestamp: {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            ));
            let _ = send_alert(
                smtp,
                &settings.wordpress.default_admin_email,
                &subject,
                &body,
            )
            .await;
        }
    }

    Ok(unhealthy_reports)
}
