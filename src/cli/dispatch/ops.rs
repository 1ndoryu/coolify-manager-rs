/* sentinel-disable-file limite-lineas: dispatcher central de ops CLI.
 * En este bloque solo se añadieron light-backup/light-restore; dividir el switch
 * completo del manager es una deuda separada del contrato funcional 245A-9. */
use crate::cli::Command;

use coolify_manager::commands;
use coolify_manager::error::CoolifyError;

use std::path::Path;

pub(super) async fn dispatch_ops_commands(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        command @ (Command::Redeploy { .. }
        | Command::FixDbAuth { .. }
        | Command::DeployWebsocket { .. }
        | Command::RunScript { .. }
        | Command::Smtp { .. }
        | Command::Migrate { .. }
        | Command::SwitchDns { .. }
        | Command::Audit { .. }
        | Command::AuditControlPlane { .. }
        | Command::AuditSecurity { .. }
        | Command::AuditRedisLatency { .. }
        | Command::CoolifyControlPlane { .. }
        | Command::InstallCoolify { .. }
        | Command::BootstrapTargetLight { .. }
        | Command::ProvisionStatic { .. }
        | Command::InventoryLight { .. }
        | Command::LightBackup { .. }
        | Command::LightRestore { .. }
        | Command::LightSite { .. }
        | Command::UninstallCoolify { .. }
        | Command::PurgeDockerHost { .. }) => dispatch_platform_ops(command, config_path).await,
        command @ (Command::HardenSsh { .. }
        | Command::EnforceHostSecurity { .. }
        | Command::Tailscale { .. }) => dispatch_security_ops(command, config_path).await,
        command @ (Command::OptimizeHost { .. }
        | Command::MaintainHost { .. }
        | Command::CheckMaintenanceWindow { .. }
        | Command::ScheduleMaintenance { .. }) => dispatch_host_ops(command, config_path).await,
        _ => unreachable!("grupo ops invalido"),
    }
}

async fn dispatch_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        command @ (Command::Redeploy { .. }
        | Command::FixDbAuth { .. }
        | Command::DeployWebsocket { .. }
        | Command::RunScript { .. }
        | Command::Smtp { .. }
        | Command::Migrate { .. }
        | Command::SwitchDns { .. }) => dispatch_site_platform_ops(command, config_path).await,
        command @ (Command::Audit { .. }
        | Command::AuditControlPlane { .. }
        | Command::AuditSecurity { .. }
        | Command::AuditRedisLatency { .. }
        | Command::CoolifyControlPlane { .. }
        | Command::InstallCoolify { .. }
        | Command::BootstrapTargetLight { .. }
        | Command::ProvisionStatic { .. }
        | Command::InventoryLight { .. }
        | Command::LightBackup { .. }
        | Command::LightRestore { .. }
        | Command::LightSite { .. }
        | Command::UninstallCoolify { .. }
        | Command::PurgeDockerHost { .. }) => {
            dispatch_coolify_platform_ops(command, config_path).await
        }
        _ => unreachable!("grupo platform ops invalido"),
    }
}

async fn dispatch_site_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::Redeploy { name, skip_backup } => {
            commands::redeploy::execute(config_path, &name, skip_backup).await
        }
        Command::FixDbAuth { name, dry_run } => {
            commands::fix_db_auth::execute(config_path, &name, dry_run).await
        }
        Command::DeployWebsocket { name } => {
            commands::deploy_websocket::execute(config_path, &name).await
        }
        Command::RunScript {
            name,
            file,
            interpreter,
            target,
            args,
        } => {
            commands::run_script::execute(
                config_path,
                &name,
                &file,
                interpreter.as_deref(),
                &target,
                args.as_deref(),
            )
            .await
        }
        Command::Smtp {
            name,
            all,
            test,
            test_email,
            status,
        } => {
            commands::setup_smtp::execute(
                config_path,
                name.as_deref(),
                all,
                test,
                test_email.as_deref(),
                status,
            )
            .await
        }
        Command::Migrate {
            name,
            target,
            dry_run,
            switch_dns,
        } => {
            commands::migrate_site::execute(config_path, &name, &target, dry_run, switch_dns).await
        }
        Command::SwitchDns {
            name,
            target,
            ip,
            dry_run,
        } => {
            commands::switch_dns::execute(
                config_path,
                &name,
                target.as_deref(),
                ip.as_deref(),
                dry_run,
            )
            .await
        }
        _ => unreachable!("grupo site platform ops invalido"),
    }
}

/* [245A-9] El dispatcher central del manager sigue concentrando muchos comandos.
 * En este bloque solo se incorporan light-backup/light-restore. */
// sentinel-disable-next-line limite-lineas
async fn dispatch_coolify_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::Audit { target } => {
            commands::audit_vps::execute(config_path, target.as_deref()).await
        }
        Command::AuditControlPlane {
            target,
            since,
            repair,
        } => {
            commands::audit_control_plane::execute(config_path, target.as_deref(), &since, repair)
                .await
        }
        Command::AuditSecurity { target } => {
            commands::audit_security::execute(config_path, target.as_deref()).await
        }
        Command::AuditRedisLatency {
            target,
            slowlog_count,
        } => {
            commands::audit_redis_latency::execute(config_path, target.as_deref(), slowlog_count)
                .await
        }
        Command::CoolifyControlPlane {
            target,
            action,
            include_proxy,
        } => {
            commands::coolify_control_plane::execute(config_path, &target, &action, include_proxy)
                .await
        }
        Command::InstallCoolify { target } => {
            commands::install_coolify::execute(config_path, &target).await
        }
        command @ (Command::BootstrapTargetLight { .. }
        | Command::ProvisionStatic { .. }
        | Command::InventoryLight { .. }
        | Command::LightBackup { .. }
        | Command::LightRestore { .. }
        | Command::LightSite { .. }) => {
            dispatch_lightweight_platform_ops(command, config_path).await
        }
        Command::UninstallCoolify {
            target,
            purge_data,
            dry_run,
        } => commands::uninstall_coolify::execute(config_path, &target, purge_data, dry_run).await,
        Command::PurgeDockerHost {
            target,
            all_data,
            dry_run,
        } => commands::purge_docker_host::execute(config_path, &target, all_data, dry_run).await,
        _ => unreachable!("grupo coolify platform ops invalido"),
    }
}

async fn dispatch_lightweight_platform_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::BootstrapTargetLight { target, dry_run } => {
            commands::bootstrap_target_light::execute(config_path, &target, dry_run).await
        }
        Command::ProvisionStatic {
            target,
            site,
            fqdn,
            access_user,
            access_password,
            json,
        } => {
            commands::provision_static::execute(
                config_path,
                &target,
                &site,
                fqdn.as_deref(),
                access_user.as_deref(),
                access_password.as_deref(),
                json,
            )
            .await
        }
        Command::InventoryLight { target, json } => {
            commands::inventory_light::execute(config_path, &target, json).await
        }
        Command::LightBackup {
            target,
            site,
            tier,
            label,
            list,
            json,
        } => {
            commands::light_backup::execute(
                config_path,
                &target,
                &site,
                &tier,
                label.as_deref(),
                list,
                json,
            )
            .await
        }
        Command::LightRestore {
            target,
            site,
            backup_id,
            access_password,
            skip_safety_snapshot,
            json,
        } => {
            commands::light_restore::execute(
                config_path,
                &target,
                &site,
                &backup_id,
                access_password.as_deref(),
                skip_safety_snapshot,
                json,
            )
            .await
        }
        Command::LightSite {
            target,
            site,
            action,
            fqdn,
            access_user,
            access_password,
            delete_volumes,
            json,
        } => {
            commands::light_site::execute(
                config_path,
                &target,
                &site,
                &action,
                fqdn.as_deref(),
                access_user.as_deref(),
                access_password.as_deref(),
                delete_volumes,
                json,
            )
            .await
        }
        _ => unreachable!("grupo lightweight platform ops invalido"),
    }
}

async fn dispatch_security_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::HardenSsh {
            target,
            dry_run,
            apply,
        } => commands::harden_ssh::execute(config_path, &target, dry_run, apply).await,
        Command::EnforceHostSecurity {
            target,
            dry_run,
            apply,
        } => commands::enforce_host_security::execute(config_path, &target, dry_run, apply).await,
        Command::Tailscale {
            target,
            auth_key,
            auth_key_env,
            hostname,
            advertise_tags,
            accept_dns,
            probe_url,
            probe_method,
            probe_body,
        } => {
            commands::tailscale::execute(
                config_path,
                target.as_deref(),
                auth_key.as_deref(),
                auth_key_env.as_deref(),
                hostname.as_deref(),
                advertise_tags.as_deref(),
                accept_dns,
                probe_url.as_deref(),
                &probe_method,
                probe_body.as_deref(),
            )
            .await
        }
        _ => unreachable!("grupo security ops invalido"),
    }
}

async fn dispatch_host_ops(
    command: Command,
    config_path: &Path,
) -> std::result::Result<(), CoolifyError> {
    match command {
        Command::OptimizeHost {
            target,
            swap_gb,
            swappiness,
            vfs_cache_pressure,
            overcommit_memory,
            disable_thp,
            docker_live_restore,
            dry_run,
            samples,
            interval_seconds,
        } => {
            commands::optimize_host::execute(
                config_path,
                target.as_deref(),
                swap_gb,
                swappiness,
                vfs_cache_pressure,
                overcommit_memory,
                disable_thp,
                docker_live_restore,
                dry_run,
                samples,
                interval_seconds,
            )
            .await
        }
        Command::MaintainHost {
            target,
            reboot,
            dry_run,
        } => {
            commands::maintain_host::execute(config_path, target.as_deref(), reboot, dry_run).await
        }
        Command::CheckMaintenanceWindow {
            target,
            apply,
            dry_run,
            force_evaluate,
        } => {
            commands::check_maintenance_window::execute(
                config_path,
                target.as_deref(),
                apply,
                dry_run,
                force_evaluate,
            )
            .await
        }
        Command::ScheduleMaintenance {
            target,
            dry_run,
            remove,
        } => commands::schedule_maintenance::execute(config_path, &target, dry_run, remove).await,
        _ => unreachable!("grupo host ops invalido"),
    }
}
