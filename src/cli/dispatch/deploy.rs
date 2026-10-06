use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_deploy_commands(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::New { .. } => dispatch_new_site(command, config_path).await,
        Command::Deploy { .. } => dispatch_deploy_theme(command, config_path).await,
        Command::DeployService {
            name,
            skip_build,
            seed,
            skip_compose_sync,
            skip_backup,
            image,
        } => {
            commands::deploy_service::execute(
                config_path,
                &name,
                skip_build,
                seed,
                skip_compose_sync,
                skip_backup,
                image.as_deref(),
            )
            .await
        }
        Command::BuildLaptop {
            name,
            dockerfile,
            target,
            tag,
            keep_tarball,
            docker_bin,
        } => {
            dispatch_build_laptop(
                config_path,
                name,
                dockerfile,
                target,
                tag,
                keep_tarball,
                docker_bin,
            )
            .await
        }
        Command::DeleteSite { .. }
        | Command::SetCompose { .. }
        | Command::OfficialDeploy { .. }
        | Command::SetBuildMode { .. }
        | Command::Restart { .. }
        | Command::Stop { .. }
        | Command::Backup { .. }
        | Command::Restore { .. }
        | Command::RestorePgData { .. }
        | Command::Health { .. } => dispatch_deploy_lifecycle(command, config_path).await,
        _ => unreachable!("grupo deploy invalido"),
    }
}

/* [06AA-4] Rama deploy-theme de dispatch_deploy_commands. Extraída del
 * match (funcion-larga-rs). */
async fn dispatch_deploy_theme(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let Command::Deploy {
        name,
        glory_branch,
        library_branch,
        update,
        skip_react,
        force,
        skip_backup,
    } = command
    else {
        unreachable!("grupo deploy invalido")
    };
    commands::deploy_theme::execute(&commands::deploy_theme::ParamsDeployTheme {
        config_path,
        site_name: &name,
        glory_branch: glory_branch.as_deref(),
        library_branch: library_branch.as_deref(),
        update,
        skip_react,
        force,
        skip_backup,
    })
    .await
}

/* [06AA-4] Rama new-site de dispatch_deploy_commands: construye los
 * Params y delega. Extraída del match (funcion-larga-rs). Recibe el Command
 * completo para no multiplicar parámetros (parametros-excesivos-rs). */
async fn dispatch_new_site(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    let Command::New {
        name,
        domain,
        glory_branch,
        library_branch,
        template,
        target,
        repo_url,
        app_bin,
        frontend_dir,
        image,
        build_mode,
        skip_theme,
        skip_cache,
    } = command
    else {
        unreachable!("grupo deploy invalido")
    };
    commands::new_site::execute(&commands::new_site::ParamsNewSite {
        config_path,
        site_name: &name,
        domain: &domain,
        glory_branch: &glory_branch,
        library_branch: &library_branch,
        template: &template,
        target_name: target.as_deref(),
        repo_url: repo_url.as_deref(),
        app_bin: app_bin.as_deref(),
        frontend_dir: frontend_dir.as_deref(),
        image: image.as_deref(),
        build_mode: &build_mode,
        skip_theme,
        skip_cache,
    })
    .await
}

/* [06AA-4] Rama build-laptop de dispatch_deploy_commands: modo sitio
 * (--name) o modo fichero (--dockerfile + --tag). Extraída del match
 * (funcion-larga-rs); sin cambios de comportamiento. */
async fn dispatch_build_laptop(
    config_path: &Path,
    name: Option<String>,
    dockerfile: Option<String>,
    target: Option<String>,
    tag: Option<String>,
    keep_tarball: bool,
    docker_bin: String,
) -> std::result::Result<(), CoolifyError> {
    match (name, dockerfile) {
        (Some(site), None) => {
            commands::build_laptop::execute(&commands::build_laptop::ParamsBuildLaptop {
                config_path,
                site_name: &site,
                tag: tag.as_deref(),
                keep_tarball,
                docker_bin: &docker_bin,
            })
            .await
            .map(|tag| {
                println!("Imagen lista: {tag}");
            })
        }
        /* [309A-1/F3] Modo fichero: Dockerfile suelto + tag explícito
         * (Kamples no tiene repo/branch del que derivar sha). */
        (None, Some(df)) => {
            let file_tag = tag.as_deref().ok_or_else(|| {
                CoolifyError::Validation(
                    "build-laptop --dockerfile requiere --tag explícito (p. ej. cm-local/kamples:manual001)".to_string(),
                )
            })?;
            commands::build_laptop::execute_file(&commands::build_laptop::ParamsBuildLaptopFile {
                config_path,
                dockerfile: &df,
                tag: file_tag,
                target: target.as_deref(),
                keep_tarball,
                docker_bin: &docker_bin,
            })
            .await
        }
        _ => Err(CoolifyError::Validation(
            "build-laptop: usa --name SITIO (modo sitio) o --dockerfile PATH + --tag TAG (modo fichero), no ambos ni ninguno".to_string(),
        )),
    }
}

/* Subgrupo ciclo de vida: borrar, reiniciar, backup/restore y salud. */
async fn dispatch_deploy_lifecycle(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::DeleteSite {
            name,
            confirm,
            dry_run,
        } => commands::delete_site::execute(config_path, &name, &confirm, dry_run).await,
        Command::SetBuildMode { name, mode } => {
            commands::set_build_mode::execute(config_path, &name, &mode).await
        }
        Command::SetCompose {
            name,
            compose_file,
            stdin,
            dry_run,
        } => {
            let source = match (compose_file, stdin) {
                (Some(path), false) => commands::set_compose::ComposeSource::File(path.into()),
                (None, true) => commands::set_compose::ComposeSource::Stdin,
                _ => {
                    return Err(CoolifyError::Validation(
                        "set-compose requiere --compose-file <path> o --stdin (excluyentes)".into(),
                    ));
                }
            };
            commands::set_compose::execute(config_path, &name, &source, dry_run).await
        }
        Command::OfficialDeploy { name, dry_run } => {
            commands::official_deploy::execute(&commands::official_deploy::ParamsOfficialDeploy {
                config_path,
                site_name: &name,
                dry_run,
            })
            .await
        }
        Command::Restart {
            name,
            all,
            only_db,
            only_wordpress,
        } => {
            commands::restart_site::execute(
                config_path,
                name.as_deref(),
                all,
                only_db,
                only_wordpress,
            )
            .await
        }
        Command::Stop { name } => commands::stop_site::execute(config_path, &name).await,
        Command::Backup {
            name,
            tier,
            label,
            list,
        } => {
            commands::backup_site::execute(config_path, &name, &tier, label.as_deref(), list).await
        }
        Command::Restore {
            name,
            backup_id,
            skip_safety_snapshot,
        } => {
            commands::restore_backup::execute(config_path, &name, &backup_id, skip_safety_snapshot)
                .await
        }
        Command::RestorePgData {
            name,
            file,
            database,
            skip_safety_snapshot,
        } => {
            commands::restore_pg_data::execute(
                config_path,
                &name,
                &file,
                database.as_deref(),
                skip_safety_snapshot,
            )
            .await
        }
        Command::Health {
            name,
            all,
            alert,
            repair,
        } => {
            commands::health_check::execute(config_path, name.as_deref(), all, alert, repair).await
        }
        _ => unreachable!("grupo deploy invalido"),
    }
}
