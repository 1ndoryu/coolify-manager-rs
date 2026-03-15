/*
 * schedule-backup — registra tareas programadas de backup en Windows Task Scheduler.
 * Crea una tarea diaria y una semanal por cada sitio con backups habilitados.
 * Las tareas se escalonan para no colapsar la red cuando hay multiples sitios.
 */

use crate::config::Settings;
use crate::error::CoolifyError;

use std::path::Path;
use std::process::Command;

const DAILY_BASE_HOUR: u32 = 3;
const WEEKLY_BASE_HOUR: u32 = 4;
const STAGGER_MINUTES: u32 = 15;
const WEEKLY_DAY: &str = "SUN";

pub async fn execute(
    config_path: &Path,
    remove: bool,
) -> std::result::Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let exe_path = resolve_exe_path()?;
    let config_arg = config_path.display().to_string();

    let sites_with_backup: Vec<_> = settings
        .sitios
        .iter()
        .filter(|site| site.backup_policy.enabled)
        .collect();

    if sites_with_backup.is_empty() {
        println!("No hay sitios con backups habilitados.");
        return Ok(());
    }

    if remove {
        return remove_all_tasks(&sites_with_backup);
    }

    println!("Registrando {} sitio(s) en Task Scheduler...", sites_with_backup.len());

    for (index, site) in sites_with_backup.iter().enumerate() {
        let stagger = index as u32 * STAGGER_MINUTES;
        let daily_hour = DAILY_BASE_HOUR + stagger / 60;
        let daily_minute = stagger % 60;
        let weekly_hour = WEEKLY_BASE_HOUR + stagger / 60;
        let weekly_minute = stagger % 60;

        let daily_task_name = format!("CoolifyManager-Backup-Daily-{}", site.nombre);
        let weekly_task_name = format!("CoolifyManager-Backup-Weekly-{}", site.nombre);

        let daily_command = format!(
            "\"{}\" --config \"{}\" backup --name {} --tier daily",
            exe_path, config_arg, site.nombre,
        );
        let weekly_command = format!(
            "\"{}\" --config \"{}\" backup --name {} --tier weekly",
            exe_path, config_arg, site.nombre,
        );

        register_daily_task(
            &daily_task_name,
            &daily_command,
            daily_hour,
            daily_minute,
        )?;
        println!(
            "  [daily]  {} -> {:02}:{:02} cada dia (max {} copias)",
            site.nombre, daily_hour, daily_minute, site.backup_policy.daily_keep,
        );

        register_weekly_task(
            &weekly_task_name,
            &weekly_command,
            weekly_hour,
            weekly_minute,
        )?;
        println!(
            "  [weekly] {} -> {:02}:{:02} cada {} (max {} copias)",
            site.nombre, weekly_hour, weekly_minute, WEEKLY_DAY, site.backup_policy.weekly_keep,
        );
    }

    println!("\nTareas programadas registradas. Verificar con: schtasks /query /tn \"CoolifyManager-*\"");
    Ok(())
}

fn resolve_exe_path() -> std::result::Result<String, CoolifyError> {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .map_err(|error| CoolifyError::Validation(format!("No se pudo determinar la ruta del ejecutable: {error}")))
}

fn register_daily_task(
    task_name: &str,
    command: &str,
    hour: u32,
    minute: u32,
) -> std::result::Result<(), CoolifyError> {
    let start_time = format!("{hour:02}:{minute:02}");
    run_schtasks(&[
        "/create",
        "/tn", task_name,
        "/tr", command,
        "/sc", "daily",
        "/st", &start_time,
        "/f",
        "/rl", "HIGHEST",
    ])
}

fn register_weekly_task(
    task_name: &str,
    command: &str,
    hour: u32,
    minute: u32,
) -> std::result::Result<(), CoolifyError> {
    let start_time = format!("{hour:02}:{minute:02}");
    run_schtasks(&[
        "/create",
        "/tn", task_name,
        "/tr", command,
        "/sc", "weekly",
        "/d", WEEKLY_DAY,
        "/st", &start_time,
        "/f",
        "/rl", "HIGHEST",
    ])
}

fn remove_all_tasks(
    sites: &[&crate::domain::SiteConfig],
) -> std::result::Result<(), CoolifyError> {
    for site in sites {
        let daily_name = format!("CoolifyManager-Backup-Daily-{}", site.nombre);
        let weekly_name = format!("CoolifyManager-Backup-Weekly-{}", site.nombre);

        let _ = run_schtasks(&["/delete", "/tn", &daily_name, "/f"]);
        let _ = run_schtasks(&["/delete", "/tn", &weekly_name, "/f"]);
        println!("Eliminadas tareas de '{}'", site.nombre);
    }
    Ok(())
}

fn run_schtasks(args: &[&str]) -> std::result::Result<(), CoolifyError> {
    let output = Command::new("schtasks")
        .args(args)
        .output()
        .map_err(|error| CoolifyError::Validation(format!("No se pudo ejecutar schtasks: {error}")))?;

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
