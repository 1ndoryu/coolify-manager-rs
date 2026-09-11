use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

/* [04A-1] M8: Verificar que los env vars críticos están presentes en el contenedor.
 * Resuelve E12 (secrets no inyectados por Coolify async worker). */
pub(crate) async fn verify_container_env_vars(
    ssh: &SshClient,
    _site_name: &str,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    /* Variables críticas que TODOS los sitios Rust necesitan */
    let critical_vars = ["DATABASE_URL", "JWT_SECRET"];

    let cmd = format!(
        "cd {} && docker compose exec -T {} printenv 2>/dev/null | grep -c ''",
        service_dir, compose_service
    );
    let env_count = ssh.execute(&cmd).await;
    match env_count {
        Ok(out) if out.success() => {
            let count: u32 = out.stdout.trim().parse().unwrap_or(0);
            if count == 0 {
                tracing::warn!(
                    "M8: Contenedor '{}' tiene 0 env vars — Coolify no inyectó secrets",
                    compose_service
                );
            }
        }
        _ => {
            tracing::warn!(
                "M8: No se pudo verificar env vars del contenedor '{}'",
                compose_service
            );
        }
    }

    /* Verificar vars críticas individualmente */
    for var in &critical_vars {
        let check = format!(
            "cd {} && docker compose exec -T {} printenv {} 2>/dev/null",
            service_dir, compose_service, var
        );
        match ssh.execute(&check).await {
            Ok(r) if r.stdout.trim().is_empty() => {
                tracing::warn!(
                    "M8: Variable {} no encontrada en contenedor '{}'",
                    var,
                    compose_service
                );
            }
            Err(_) => {
                tracing::warn!("M8: Error al verificar {} en '{}'", var, compose_service);
            }
            _ => {}
        }
    }

    Ok(())
}

/* [04A-1] M9: Verificar que los volúmenes nombrados están montados en el contenedor.
 * Resuelve E9 (volúmenes huérfanos sin attach post-crash). */
pub(crate) async fn verify_container_volumes(
    ssh: &SshClient,
    _site_name: &str,
    service_dir: &str,
    compose_service: &str,
) -> std::result::Result<(), CoolifyError> {
    let expected_mounts = ["/app/uploads"];

    for mount in &expected_mounts {
        let check = format!(
            "cd {} && docker compose exec -T {} test -d {} 2>&1 && echo OK",
            service_dir, compose_service, mount
        );
        match ssh.execute(&check).await {
            Ok(r) if r.stdout.contains("OK") => {
                tracing::debug!("M9: {} montado en '{}'", mount, compose_service);
            }
            _ => {
                tracing::warn!(
                    "M9: {} NO encontrado en contenedor '{}'",
                    mount,
                    compose_service
                );
            }
        }
    }

    Ok(())
}

/* [incident-2026-07-01] M10: Verificar que PostgreSQL tiene su volumen de datos persistente.
 * Si el contenedor postgres no tiene /var/lib/postgresql/data montado en un named volume,
 * los datos se pierden al recrear el contenedor. Esto causó pérdida total de datos en
 * nakomi.studio el 2026-07-01.
 * Verifica inspeccionando los mounts del contenedor postgres via docker inspect. */
pub(crate) async fn verify_postgres_data_volume(
    ssh: &SshClient,
    stack_uuid: &str,
    _service_dir: &str,
) -> std::result::Result<(), CoolifyError> {
    let postgres_container = format!("postgres-{}", stack_uuid);

    /* Verificar que el contenedor postgres existe */
    let check_exists = format!(
        "docker inspect --format '{{{{.State.Status}}}}' {} 2>/dev/null",
        postgres_container
    );
    match ssh.execute(&check_exists).await {
        Ok(r) if !r.stdout.trim().is_empty() && !r.stdout.contains("Error") => {
            /* Contenedor existe — verificar que tiene volumen persistente montado */
            let check_mounts = format!(
                "docker inspect --format '{{{{range .Mounts}}}}{{{{.Name}}}}:{{{{.Destination}}}} {{{{end}}}}' {} 2>/dev/null",
                postgres_container
            );
            match ssh.execute(&check_mounts).await {
                Ok(mounts) if mounts.stdout.contains("/var/lib/postgresql/data") => {
                    tracing::debug!(
                        "M10: PostgreSQL volumen de datos OK en '{}'",
                        postgres_container
                    );
                }
                Ok(mounts) => {
                    tracing::error!(
                        "M10: CRITICO — PostgreSQL '{}' NO tiene volumen persistente en /var/lib/postgresql/data. \
                         Mounts actuales: '{}'. Los datos se perderán al recrear el contenedor.",
                        postgres_container,
                        mounts.stdout.trim()
                    );
                    eprintln!(
                        "⚠️  M10: PostgreSQL sin volumen persistente — riesgo de pérdida de datos"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "M10: No se pudo verificar mounts de '{}': {}",
                        postgres_container,
                        e
                    );
                }
            }
        }
        _ => {
            tracing::debug!(
                "M10: Contenedor postgres '{}' no existe aún (será creado en swap)",
                postgres_container
            );
        }
    }

    Ok(())
}
