/*
 * install-backups — Instala el script de backup automático en el VPS.
 * Sube backup-server.sh a /usr/local/bin/, genera /etc/backup-sites.conf
 * desde settings.json, y configura crontab root.
 * Los backups corren en el servidor, sin dependencia del PC Windows.
 * El script auto-descubre containers postgres/mariadb — zero hardcoding.
 *
 * Uso:
 *   coolify-manager install-backups                    # Instalar en VPS default
 *   coolify-manager install-backups --target standby   # Instalar en target específico
 *   coolify-manager install-backups --dry-run          # Solo mostrar qué haría
 *   coolify-manager install-backups --uninstall        # Remover script y crontab
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::infra::ssh_client::SshClient;

use base64::Engine as _;
use std::path::Path;

const REMOTE_SCRIPT_PATH: &str = "/usr/local/bin/backup-server.sh";
const REMOTE_CONFIG_PATH: &str = "/etc/backup-sites.conf";
const REMOTE_LOG_PATH: &str = "/data/backups/backup.log";

/// Contenido del script de backup (embebido para no depender de archivo local).
const BACKUP_SCRIPT_CONTENT: &str = include_str!("../../scripts/backup-server.sh");

pub async fn execute(
    config_path: &Path,
    target_name: Option<&str>,
    dry_run: bool,
    uninstall: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;

    /* Resolver target VPS */
    let vps_config = match target_name {
        Some(name) => {
            let target = settings.get_target(name)?;
            &target.vps
        }
        None => &settings.vps,
    };

    if dry_run {
        println!("[dry-run] Script backup-server.sh:");
        println!("  Tamaño: {} bytes", BACKUP_SCRIPT_CONTENT.len());
        println!("  Destino: {REMOTE_SCRIPT_PATH}");
        println!("  Crontab: 0 3 * * * {REMOTE_SCRIPT_PATH}");
        println!("  Log: {REMOTE_LOG_PATH}");
        println!("  Directorio backups: /data/backups/");
        println!();
        println!("[dry-run] Config generado desde settings.json:");
        let config_content = generate_sites_config(&settings);
        for line in config_content.lines() {
            println!("  {line}");
        }
        return Ok(());
    }

    let mut ssh = SshClient::from_vps(vps_config);
    ssh.connect().await?;
    println!("Conectado a {}@{}", ssh.user(), vps_config.ip);

    if uninstall {
        return uninstall_backups(&ssh).await;
    }

    /* 1. Crear directorio de backups */
    println!("[1/5] Creando directorio /data/backups/...");
    let mkdir = ssh.execute("mkdir -p /data/backups").await?;
    if !mkdir.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo crear /data/backups/: {}",
            mkdir.stderr.trim()
        )));
    }

    /* 2. Subir script via base64 */
    println!("[2/5] Subiendo backup-server.sh...");
    let encoded = base64::engine::general_purpose::STANDARD.encode(BACKUP_SCRIPT_CONTENT.as_bytes());
    let write_cmd = format!(
        "echo '{}' | base64 -d > {}",
        encoded, REMOTE_SCRIPT_PATH
    );
    let write = ssh.execute(&write_cmd).await?;
    if !write.success() {
        return Err(CoolifyError::Validation(format!(
            "No se pudo escribir el script: {}",
            write.stderr.trim()
        )));
    }

    /* 3. Generar y subir config desde settings.json */
    println!("[3/5] Generando {} desde settings.json...", REMOTE_CONFIG_PATH);
    let config_content = generate_sites_config(&settings);
    if config_content.is_empty() {
        println!("  (sin overrides — el script usará defaults)");
    } else {
        let config_encoded = base64::engine::general_purpose::STANDARD.encode(config_content.as_bytes());
        let config_cmd = format!(
            "echo '{}' | base64 -d > {}",
            config_encoded, REMOTE_CONFIG_PATH
        );
        let config_write = ssh.execute(&config_cmd).await?;
        if !config_write.success() {
            return Err(CoolifyError::Validation(format!(
                "No se pudo escribir config: {}",
                config_write.stderr.trim()
            )));
        }
        println!("  ✅ {} escrito ({} sitios con overrides)", REMOTE_CONFIG_PATH,
            config_content.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).count());
    }

    /* 4. chmod +x + crontab */
    println!("[4/5] Haciendo ejecutable + configurando crontab...");
    let chmod = ssh
        .execute(&format!("chmod +x {REMOTE_SCRIPT_PATH}"))
        .await?;
    if !chmod.success() {
        return Err(CoolifyError::Validation(format!(
            "chmod fallo: {}",
            chmod.stderr.trim()
        )));
    }

    let cron_line = format!(
        "0 3 * * * {REMOTE_SCRIPT_PATH} >> {REMOTE_LOG_PATH} 2>&1"
    );

    let check_cron = ssh.execute("crontab -l 2>/dev/null || true").await?;
    if check_cron.stdout.contains(REMOTE_SCRIPT_PATH) {
        println!("  Crontab ya contiene entrada — actualizando...");
        let replace_cmd = format!(
            "crontab -l 2>/dev/null | grep -v '{}' | (cat; echo '{}') | crontab -",
            REMOTE_SCRIPT_PATH, cron_line
        );
        ssh.execute(&replace_cmd).await?;
    } else {
        let add_cmd = format!(
            "(crontab -l 2>/dev/null; echo '{}') | crontab -",
            cron_line
        );
        ssh.execute(&add_cmd).await?;
    }

    /* 5. Verificar + backup de prueba (auto-descubrimiento) */
    println!("[5/5] Verificando instalación...");
    let verify = ssh.execute("crontab -l 2>/dev/null").await?;
    if verify.stdout.contains(REMOTE_SCRIPT_PATH) {
        println!("\n✅ Instalación completa:");
        println!("   Script:  {REMOTE_SCRIPT_PATH}");
        println!("   Config:  {REMOTE_CONFIG_PATH}");
        println!("   Crontab: 0 3 * * * (diario a las 03:00 UTC)");
        println!("   Log:     {REMOTE_LOG_PATH}");
        println!("   Backups: /data/backups/{{stack_uuid}}/{{daily|weekly}}/");
    } else {
        return Err(CoolifyError::Validation(
            "No se pudo verificar el crontab después de la instalación".into(),
        ));
    }

    /* Ejecutar dry-run para verificar containers detectados */
    println!("\nAuto-descubriendo containers...");
    let dry_run = ssh
        .execute(&format!("{REMOTE_SCRIPT_PATH} --dry-run 2>&1"))
        .await;
    match dry_run {
        Ok(output) => {
            let lines: Vec<&str> = output.stdout.lines().collect();
            if lines.is_empty() {
                println!("   ⚠️ No se encontraron containers de base de datos");
            } else {
                println!("   Containers encontrados:");
                for line in &lines {
                    println!("   {line}");
                }
                /* Ejecutar primer backup real */
                println!("\nEjecutando primer backup...");
                let first_run = ssh
                    .execute(&format!("{REMOTE_SCRIPT_PATH} 2>&1"))
                    .await;
                match first_run {
                    Ok(_run_output) => {
                        let tail = ssh
                            .execute(&format!("tail -10 {REMOTE_LOG_PATH}"))
                            .await
                            .unwrap_or_default();
                        for line in tail.stdout.lines() {
                            println!("   {line}");
                        }
                    }
                    Err(e) => {
                        println!("   ⚠️ Error en primer backup: {e}");
                    }
                }
            }
        }
        Err(e) => {
            println!("   ⚠️ Error en dry-run: {e}");
        }
    }

    Ok(())
}

async fn uninstall_backups(ssh: &SshClient) -> std::result::Result<(), CoolifyError> {
    println!("Desinstalando backup automático...");

    /* Remover del crontab */
    let remove_cron = ssh
        .execute(&format!(
            "crontab -l 2>/dev/null | grep -v '{}' | crontab -",
            REMOTE_SCRIPT_PATH
        ))
        .await?;
    if remove_cron.success() {
        println!("  ✅ Entrada de crontab removida");
    }

    /* Eliminar script */
    let rm = ssh
        .execute(&format!("rm -f {REMOTE_SCRIPT_PATH}"))
        .await?;
    if rm.success() {
        println!("  ✅ Script eliminado");
    }

    /* Eliminar config */
    let rm_config = ssh
        .execute(&format!("rm -f {REMOTE_CONFIG_PATH}"))
        .await?;
    if rm_config.success() {
        println!("  ✅ Config eliminado ({REMOTE_CONFIG_PATH})");
    }

    println!("\nBackups existentes en /data/backups/ NO fueron eliminados.");
    println!("Para eliminarlos: ssh root@VPS \"rm -rf /data/backups/\"");

    Ok(())
}

/// Genera el contenido de /etc/backup-sites.conf desde settings.json.
/// Solo incluye sitios con backupPolicy.enabled=true y que tengan stackUuid.
/// Las políticas no-default se escriben como overrides.
fn generate_sites_config(settings: &Settings) -> String {
    let mut lines: Vec<String> = vec![
        "# /etc/backup-sites.conf — Generado por coolify-manager install-backups".to_string(),
        "# Formato: STACK_UUID|daily_keep|weekly_keep|max_daily_mb".to_string(),
        "# Solo se listan sitios con overrides; el resto usa defaults.".to_string(),
        "# Defaults: daily_keep=2, weekly_keep=2, max_daily_mb=500".to_string(),
        "".to_string(),
    ];

    let default_daily = 2usize;
    let default_weekly = 2usize;
    let _default_max_mb = 500u64;

    for site in &settings.sitios {
        if !site.backup_policy.enabled {
            continue;
        }
        let Some(ref uuid) = site.stack_uuid else {
            continue;
        };

        let daily = site.backup_policy.daily_keep;
        let weekly = site.backup_policy.weekly_keep;
        let max_mb: u64 = 500; /* source_paths no define max_mb; usar default */

        /* Solo escribir si tiene overrides no-default */
        let has_overrides = daily != default_daily || weekly != default_weekly;

        if has_overrides {
            lines.push(format!(
                "{}|{}|{}|{}",
                uuid, daily, weekly, max_mb
            ));
        }
    }

    /* Si no hay overrides, devolver solo los comments */
    if lines.len() <= 5 {
        return String::new();
    }

    lines.join("\n")
}
