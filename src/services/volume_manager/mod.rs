/* [124A-IMAGE404] Gestión de volúmenes persistentes.
 *
 * Coolify normaliza bind mount paths a named volumes en su API interna.
 * Esto causa que las imágenes/uploads desaparezcan después de un redeploy
 * o restart iniciado desde Coolify (UI o API), porque Coolify reescribe
 * el compose en disco con su versión procesada que usa named volumes.
 *
 * Este módulo proporciona funciones para forzar bind mounts en el compose
 * en disco, garantizando que docker compose build/up siempre use el path
 * persistente del host.
 *
 * Gotcha: Coolify procesa compose volumes así:
 *   raw: 'uploads_data:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 *   raw: '/data/uploads/studio:/app/uploads' → processed: 'UUID_uploads-data:/app/uploads'
 * Ambos formatos se normalizan al mismo named volume. No hay forma de evitarlo
 * via API. La solución es parchear el archivo en disco después de que Coolify escriba. */

pub mod compose_envs;
pub mod compose_remoto;
pub mod compose_volumenes;
pub mod envs_runtime;
pub mod montaje_ssh;
pub mod texto;
pub mod tipos;
pub mod uploads;

pub use envs_runtime::ensure_runtime_envs_in_compose;
pub use montaje_ssh::ensure_runtime_ssh_bind_mount;
pub use uploads::{
    ensure_uploads_bind_mount, ensure_uploads_host_dir, merge_current_uploads_into_host_bind,
    verify_runtime_uploads_bind_mount,
};

#[cfg(test)]
mod tests {
    use crate::services::volume_manager::compose_envs::upsert_service_environment_entries;
    use crate::services::volume_manager::compose_volumenes::ensure_service_volume_entry;
    use crate::services::volume_manager::texto::{mount_points_app_uploads_to, yaml_single_quote};

    #[test]
    fn mount_parser_accepts_expected_bind_mount() {
        assert!(mount_points_app_uploads_to(
            "/app/uploads | bind | /data/uploads/studio",
            "/data/uploads/studio"
        ));
    }

    #[test]
    fn mount_parser_rejects_named_volume() {
        assert!(!mount_points_app_uploads_to(
            "/app/uploads | volume | /var/lib/docker/volumes/demo/_data",
            "/data/uploads/studio"
        ));
    }

    #[test]
    fn upsert_service_environment_entries_inserts_missing_runtime_env() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            GLORY_ADMIN_EMAILS: 'admin@example.com'
        depends_on:
            postgres:
                condition: service_healthy
    postgres:
        image: postgres:16
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "GLORY_TEST_CHECKOUT_EMAILS".to_string(),
                "test@test.com".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert_eq!(
            sync.inserted_keys,
            vec!["GLORY_TEST_CHECKOUT_EMAILS".to_string()]
        );
        assert!(sync
            .content
            .contains("GLORY_TEST_CHECKOUT_EMAILS: 'test@test.com'"));
        assert!(sync.content.contains("depends_on:"));
    }

    #[test]
    fn upsert_service_environment_entries_does_not_duplicate_existing_key() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            GLORY_TEST_CHECKOUT_EMAILS: 'test@test.com'
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "GLORY_TEST_CHECKOUT_EMAILS".to_string(),
                "test@test.com".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert!(sync.inserted_keys.is_empty());
        assert_eq!(sync.content, compose);
    }

    #[test]
    fn upsert_service_environment_entries_updates_changed_value() {
        let compose = r#"services:
    app:
        image: demo
        environment:
            COOLIFY_VPS1_BASE_URL: 'http://66.94.100.241:8000'
"#;

        let sync = upsert_service_environment_entries(
            compose,
            "app",
            &[(
                "COOLIFY_VPS1_BASE_URL".to_string(),
                "http://coolify:8080".to_string(),
            )],
        )
        .expect("compose env sync should succeed");

        assert!(sync.inserted_keys.is_empty());
        assert_eq!(sync.updated_keys, vec!["COOLIFY_VPS1_BASE_URL".to_string()]);
        assert!(sync.content.contains("http://coolify:8080"));
        assert!(!sync.content.contains("66.94.100.241"));
    }

    #[test]
    fn ensure_service_volume_entry_adds_missing_volume() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - 'app_data:/app/data'
        environment:
            HOST: '0.0.0.0'
"#;

        let sync =
            ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
                .expect("volume sync should succeed");

        assert!(sync.changed);
        assert!(sync
            .content
            .contains("- '/root/studio-ssh:/home/appuser/.ssh'"));
        assert!(sync.content.contains("environment:"));
    }

    #[test]
    fn ensure_service_volume_entry_does_not_duplicate_existing_volume() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - '/root/studio-ssh:/home/appuser/.ssh'
"#;

        let sync =
            ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
                .expect("volume sync should succeed");

        assert!(!sync.changed);
        assert_eq!(sync.content, compose);
    }

    #[test]
    fn ensure_service_volume_entry_replaces_existing_target() {
        let compose = r#"services:
    app:
        image: demo
        volumes:
            - '/root/studio-ssh:/home/appuser/.ssh:ro'
"#;

        let sync =
            ensure_service_volume_entry(compose, "app", "/root/studio-ssh:/home/appuser/.ssh")
                .expect("volume sync should succeed");

        assert!(sync.changed);
        assert!(sync
            .content
            .contains("- '/root/studio-ssh:/home/appuser/.ssh'"));
        assert!(!sync.content.contains(":/home/appuser/.ssh:ro"));
    }

    #[test]
    fn yaml_single_quote_escapes_single_quotes() {
        assert_eq!(yaml_single_quote("it'works"), "'it''works'");
    }
}
