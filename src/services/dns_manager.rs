/*
 * DNS manager — switch y borrado de registros (Contabo/Cloudflare).
 * (split 119A-5: switch.rs, borrado.rs, nombres.rs y tipos.rs para bajar del
 * god-object de 507 lineas. Se re-exporta plano para preservar las rutas
 * `dns_manager::{switch_site_dns, delete_site_dns, delete_orphan_dns, ...}`.)
 */

mod borrado;
mod nombres;
mod switch;
mod tipos;

pub use borrado::{delete_orphan_dns, delete_site_dns};
pub use switch::switch_site_dns;
pub use tipos::{DnsDeleteAction, DnsDeleteReport, DnsSwitchAction, DnsSwitchReport};

#[cfg(test)]
mod tests {
    use super::borrado::plan_borrado_dns;
    use super::nombres::{relative_record_from_host, resolve_records_for_site};
    use super::tipos::RegistroExistente;
    use super::*;
    use crate::domain::{
        BackupPolicy, HealthCheckConfig, SiteConfig, SiteDnsConfig, SiteDnsRecord, StackTemplate,
    };

    fn sample_site(domain: &str, template: StackTemplate) -> SiteConfig {
        SiteConfig {
            nombre: "blog".to_string(),
            dominio: domain.to_string(),
            extra_domains: Vec::new(),
            target: None,
            stack_uuid: Some("stack".to_string()),
            glory_branch: "main".to_string(),
            library_branch: "main".to_string(),
            theme_name: "glory".to_string(),
            skip_react: false,
            template,
            php_config: None,
            smtp_config: None,
            disable_wp_cron: false,
            backup_policy: BackupPolicy::default(),
            health_check: HealthCheckConfig::default(),
            dns_config: None,
            repo_url: None,
            app_bin: crate::domain::default_app_bin(),
            frontend_dir: crate::domain::default_frontend_dir(),
            image_ref: None,
        }
    }

    #[test]
    fn test_relative_record_from_host() {
        assert_eq!(
            relative_record_from_host("kamples.com", "kamples.com").unwrap(),
            "@"
        );
        assert_eq!(
            relative_record_from_host("task.nakomi.studio", "nakomi.studio").unwrap(),
            "task"
        );
        assert_eq!(
            relative_record_from_host("ws.task.nakomi.studio", "nakomi.studio").unwrap(),
            "ws.task"
        );
    }

    #[test]
    fn test_resolve_records_for_kamples_adds_ws() {
        let site = sample_site("https://kamples.com", StackTemplate::Kamples);
        let dns = SiteDnsConfig {
            provider: "contabo".to_string(),
            zone: "kamples.com".to_string(),
            switch_on_migration: true,
            records: Vec::new(),
        };
        let records = resolve_records_for_site(&site, &dns).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "@");
        assert_eq!(records[1].name, "ws");
    }

    /* [B4-4] Solo se borra si el contenido == IP de la VPS. */
    #[test]
    fn test_plan_borrado_solo_si_apunta_a_la_vps() {
        use crate::domain::DnsRecordType;
        let deseados = vec![SiteDnsRecord {
            name: "cm-test-b4".to_string(),
            record_type: DnsRecordType::A,
            ttl: 300,
        }];
        let reg = |contenido: &str| RegistroExistente {
            nombre: "cm-test-b4".to_string(),
            tipo: "A".to_string(),
            contenido: contenido.to_string(),
            id: "7".to_string(),
        };
        // Apunta a la VPS → se borra (o would-delete en dry-run).
        let plan = plan_borrado_dns(&deseados, &[reg("66.94.100.241")], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "deleted");
        assert_eq!(plan[0].id_a_borrar.as_deref(), Some("7"));
        let plan = plan_borrado_dns(&deseados, &[reg("66.94.100.241")], "66.94.100.241", true);
        assert_eq!(plan[0].action.action, "would-delete");
        assert_eq!(plan[0].id_a_borrar.as_deref(), Some("7"));
        // Apunta a otra IP (migración) → se conserva.
        let plan = plan_borrado_dns(&deseados, &[reg("9.9.9.9")], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "kept-remote");
        assert_eq!(plan[0].action.value, "9.9.9.9");
        assert!(plan[0].id_a_borrar.is_none());
        // Ausente → absent, sin id.
        let plan = plan_borrado_dns(&deseados, &[], "66.94.100.241", false);
        assert_eq!(plan[0].action.action, "absent");
        assert!(plan[0].id_a_borrar.is_none());
        // Duplicado → ambiguo, no se toca.
        let plan = plan_borrado_dns(
            &deseados,
            &[reg("66.94.100.241"), reg("66.94.100.241")],
            "66.94.100.241",
            false,
        );
        assert_eq!(plan[0].action.action, "ambiguous-skipped");
        assert!(plan[0].id_a_borrar.is_none());
    }
}
