use std::path::{Path, PathBuf};

use crate::error::CoolifyError;

pub fn load_for_config(config_path: &Path) -> std::result::Result<(), CoolifyError> {
    for candidate in candidate_env_files(config_path) {
        if candidate.exists() {
            dotenvy::from_path(&candidate).map_err(|error| {
                CoolifyError::Validation(format!(
                    "No se pudo cargar {}: {error}",
                    candidate.display()
                ))
            })?;
            tracing::debug!(".env cargado desde {}", candidate.display());
            break;
        }
    }

    Ok(())
}

fn candidate_env_files(config_path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(project_root) = config_path.parent().and_then(|dir| dir.parent()) {
        candidates.push(project_root.join(".env"));
        candidates.push(project_root.join(".env.local"));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        let env_path = current_dir.join(".env");
        if !candidates.iter().any(|candidate| candidate == &env_path) {
            candidates.push(env_path);
        }

        let env_local_path = current_dir.join(".env.local");
        if !candidates
            .iter()
            .any(|candidate| candidate == &env_local_path)
        {
            candidates.push(env_local_path);
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_load_for_config_reads_project_env() {
        let unique_key = "CMRS_TEST_ENV_LOADER_KEY";
        unsafe {
            std::env::remove_var(unique_key);
        }

        let temp = tempdir().unwrap();
        let config_dir = temp.path().join("config");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("settings.json");
        fs::write(&config_path, "{}").unwrap();
        fs::write(temp.path().join(".env"), format!("{unique_key}=ok\n")).unwrap();

        load_for_config(&config_path).unwrap();

        assert_eq!(std::env::var(unique_key).unwrap(), "ok");
        unsafe {
            std::env::remove_var(unique_key);
        }
    }
}
