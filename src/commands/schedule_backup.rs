/*
 * schedule-backup — registra tareas programadas de backup en Windows Task Scheduler.
 * Crea una tarea diaria y una semanal por cada sitio con backups habilitados.
 * Las tareas se escalonan para no colapsar la red cuando hay multiples sitios.
 *
 * Para evitar el limite de 261 caracteres de schtasks /tr, se generan scripts
 * wrapper en %LOCALAPPDATA%\CoolifyManager\ y se registran esos scripts.
 */

use crate::config::Settings;
use crate::error::CoolifyError;

use std::path::{Path, PathBuf};
use std::process::Command;

const DAILY_BASE_HOUR: u32 = 3;
const WEEKLY_BASE_HOUR: u32 = 4;
const STAGGER_MINUTES: u32 = 15;
const WEEKLY_DAY: &str = "SUN";

pub async fn execute(
    config_path: &Path,
    site_name: Option<&str>,
    remove: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let exe_path = resolve_exe_path()?;
    let config_abs = std::fs::canonicalize(config_path).map_err(|e| {
        CoolifyError::Validation(format!("No se pudo resolver ruta absoluta de config: {e}"))
    })?;
    let scripts_dir = resolve_scripts_dir()?;

    let all_backup_sites: Vec<_> = settings
        .sitios
        .iter()
        .filter(|site| site.backup_policy.enabled)
        .collect();

    let sites_with_backup: Vec<_> = match site_name {
        Some(name) => {
            let found = all_backup_sites
                .into_iter()
                .find(|site| site.nombre == name)
                .ok_or_else(|| {
                    CoolifyError::Validation(format!(
                        "Sitio '{}' no encontrado o no tiene backups habilitados",
                        name
                    ))
                })?;
            vec![found]
        }
        None => all_backup_sites,
    };

    if sites_with_backup.is_empty() {
        println!("No hay sitios con backups habilitados.");
        return Ok(());
    }

    if remove {
        return remove_all_tasks(&sites_with_backup, &scripts_dir);
    }

    println!(
        "Registrando {} sitio(s) en Task Scheduler...",
        sites_with_backup.len()
    );
    println!("Scripts wrapper en: {}", scripts_dir.display());

    for (index, site) in sites_with_backup.iter().enumerate() {
        let stagger = index as u32 * STAGGER_MINUTES;
        let daily_hour = DAILY_BASE_HOUR + stagger / 60;
        let daily_minute = stagger % 60;
        let weekly_hour = WEEKLY_BASE_HOUR + stagger / 60;
        let weekly_minute = stagger % 60;

        let daily_task_name = format!("CoolifyManager-Backup-Daily-{}", site.nombre);
        let weekly_task_name = format!("CoolifyManager-Backup-Weekly-{}", site.nombre);

        let daily_script = write_wrapper_script(
            &scripts_dir,
            &exe_path,
            &config_abs,
            &site.nombre,
            "daily",
            &daily_task_name,
        )?;
        register_daily_task(&daily_task_name, &daily_script, daily_hour, daily_minute)?;
        println!(
            "  [daily]  {} -> {:02}:{:02} cada dia (max {} copias)",
            site.nombre, daily_hour, daily_minute, site.backup_policy.daily_keep,
        );

        let weekly_script = write_wrapper_script(
            &scripts_dir,
            &exe_path,
            &config_abs,
            &site.nombre,
            "weekly",
            &weekly_task_name,
        )?;
        register_weekly_task(
            &weekly_task_name,
            &weekly_script,
            weekly_hour,
            weekly_minute,
        )?;
        println!(
            "  [weekly] {} -> {:02}:{:02} cada {} (max {} copias)",
            site.nombre, weekly_hour, weekly_minute, WEEKLY_DAY, site.backup_policy.weekly_keep,
        );
    }

    println!(
        "\nTareas programadas registradas. Verificar con: schtasks /query /tn \"CoolifyManager-*\""
    );
    Ok(())
}

fn resolve_exe_path() -> std::result::Result<String, CoolifyError> {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .map_err(|error| {
            CoolifyError::Validation(format!(
                "No se pudo determinar la ruta del ejecutable: {error}"
            ))
        })
}

/// Directorio corto para scripts wrapper (evita limite 261 chars de schtasks).
fn resolve_scripts_dir() -> std::result::Result<PathBuf, CoolifyError> {
    let local_app_data = std::env::var("LOCALAPPDATA")
        .map_err(|_| CoolifyError::Validation("Variable LOCALAPPDATA no disponible".to_string()))?;
    let dir = PathBuf::from(local_app_data).join("CoolifyManager");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| {
            CoolifyError::Validation(format!("No se pudo crear {}: {e}", dir.display()))
        })?;
    }
    Ok(dir)
}

/// Genera un .bat wrapper que invoca el exe con los argumentos correctos.
fn write_wrapper_script(
    scripts_dir: &Path,
    exe_path: &str,
    config_path: &Path,
    site_name: &str,
    tier: &str,
    task_name: &str,
) -> std::result::Result<String, CoolifyError> {
    let script_path = scripts_dir.join(format!("{task_name}.bat"));
    let content = format!(
        "@echo off\r\n\"{exe}\" --config \"{config}\" backup --name {site} --tier {tier}\r\n",
        exe = exe_path,
        config = config_path.display(),
        site = site_name,
        tier = tier,
    );
    std::fs::write(&script_path, &content).map_err(|e| {
        CoolifyError::Validation(format!(
            "No se pudo escribir script {}: {e}",
            script_path.display()
        ))
    })?;
    Ok(script_path.display().to_string())
}

fn register_daily_task(
    task_name: &str,
    script_path: &str,
    hour: u32,
    minute: u32,
) -> std::result::Result<(), CoolifyError> {
    let start_time = format!("{hour:02}:{minute:02}");
    run_schtasks(&[
        "/create",
        "/tn",
        task_name,
        "/tr",
        script_path,
        "/sc",
        "daily",
        "/st",
        &start_time,
        "/f",
    ])
}

fn register_weekly_task(
    task_name: &str,
    script_path: &str,
    hour: u32,
    minute: u32,
) -> std::result::Result<(), CoolifyError> {
    let start_time = format!("{hour:02}:{minute:02}");
    run_schtasks(&[
        "/create",
        "/tn",
        task_name,
        "/tr",
        script_path,
        "/sc",
        "weekly",
        "/d",
        WEEKLY_DAY,
        "/st",
        &start_time,
        "/f",
    ])
}

fn remove_all_tasks(
    sites: &[&crate::domain::SiteConfig],
    scripts_dir: &Path,
) -> std::result::Result<(), CoolifyError> {
    for site in sites {
        let daily_name = format!("CoolifyManager-Backup-Daily-{}", site.nombre);
        let weekly_name = format!("CoolifyManager-Backup-Weekly-{}", site.nombre);

        let _ = run_schtasks(&["/delete", "/tn", &daily_name, "/f"]);
        let _ = run_schtasks(&["/delete", "/tn", &weekly_name, "/f"]);

        /* Eliminar scripts wrapper */
        let _ = std::fs::remove_file(scripts_dir.join(format!("{daily_name}.bat")));
        let _ = std::fs::remove_file(scripts_dir.join(format!("{weekly_name}.bat")));

        println!("Eliminadas tareas de '{}'", site.nombre);
    }
    Ok(())
}

fn run_schtasks(args: &[&str]) -> std::result::Result<(), CoolifyError> {
    let output = Command::new("schtasks")
        .args(args)
        .output()
        .map_err(|error| {
            CoolifyError::Validation(format!("No se pudo ejecutar schtasks: {error}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoolifyError::Validation(format!(
            "schtasks fallo (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim(),
        )));
    }

    Ok(())
}
