use crate::error::CoolifyError;

/* [04A-1] M4: Backup del compose antes de sobrescribir.
 * Resuelve E6 (sin compose backup) y E11 (Coolify overwrite sin rollback).
 * Guarda el compose actual en ~/.coolify-manager/compose-backups/{site}/
 * con timestamp + hash. Mantiene solo los últimos 5 por sitio. */
pub(crate) fn backup_compose_locally(
    site_name: &str,
    compose: &str,
) -> std::result::Result<(), CoolifyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CoolifyError::Validation("No se pudo determinar HOME directory".into()))?;
    let backup_dir = home
        .join(".coolify-manager")
        .join("compose-backups")
        .join(site_name);
    std::fs::create_dir_all(&backup_dir)?;

    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let hash = simple_hash(compose);
    let filename = format!("compose-{}-{}.yml", timestamp, &hash[..8]);
    let path = backup_dir.join(&filename);

    std::fs::write(&path, compose)?;

    /* Mantener solo los últimos 5 backups */
    let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("compose-"))
        .collect();
    backups.sort_by_key(|e| e.file_name());
    while backups.len() > 5 {
        if let Some(old) = backups.first() {
            let _ = std::fs::remove_file(old.path());
        }
        backups.remove(0);
    }

    tracing::info!("Compose backup guardado en {}", path.display());
    Ok(())
}

/* [04A-1] E11: Lee el último compose backup para rollback automático.
 * Busca en ~/.coolify-manager/compose-backups/{site_name}/ y retorna
 * el contenido del archivo más reciente (ordenado por nombre = timestamp). */
pub(crate) fn read_latest_compose_backup(
    site_name: &str,
) -> std::result::Result<Option<String>, CoolifyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CoolifyError::Validation("No se pudo determinar HOME directory".into()))?;
    let backup_dir = home
        .join(".coolify-manager")
        .join("compose-backups")
        .join(site_name);

    if !backup_dir.exists() {
        return Ok(None);
    }

    let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("compose-"))
        .collect();
    backups.sort_by_key(|e| e.file_name());

    match backups.last() {
        Some(entry) => {
            let content = std::fs::read_to_string(entry.path())?;
            tracing::info!(
                "E11: Backup encontrado para '{}': {}",
                site_name,
                entry.file_name().to_string_lossy()
            );
            Ok(Some(content))
        }
        None => Ok(None),
    }
}

/* Hash simple para identificar versiones de compose (no criptográfico). */
fn simple_hash(s: &str) -> String {
    let mut hash: u32 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    format!("{:08x}", hash)
}
