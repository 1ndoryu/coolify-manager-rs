/*
 * delete-dns — Elimina un registro DNS huérfano (sin sitio en settings).
 *
 * [B4-4] Los borrados de stacks dejaban residuos DNS apuntando a una IP sin
 * stack detrás (`A cm-test-b4`, `A cm-test-119a2`). Este comando los retira
 * con la misma regla de seguridad que `delete-site`: solo se borra si el
 * registro apunta a la IP esperada (VPS principal o --ip explícita); si
 * apunta a otra IP se conserva (posible migración) y si hay duplicados no se
 * toca nada. Sin --dry-run ni --confirm no hay ejecución: exige confirmación
 * tipada con el FQDN, igual que delete-site.
 */

use crate::config::Settings;
use crate::error::CoolifyError;
use crate::services::dns_manager;

use clap::Args;
use std::path::Path;

#[derive(Args)]
pub struct DeleteDnsArgs {
    /// Nombre del proveedor DNS en settings.json (dnsProviders[].name)
    #[arg(long)]
    pub provider: String,

    /// Zona DNS (p.ej. wandori.us)
    #[arg(long)]
    pub zone: String,

    /// Registro: nombre relativo (cm-test-b4) o FQDN (cm-test-b4.wandori.us)
    #[arg(long)]
    pub name: String,

    /// IP esperada: solo se borra si el registro apunta aquí (defecto: VPS principal)
    #[arg(long)]
    pub ip: Option<String>,

    /// Confirmación tipada: FQDN del registro (exigida salvo --dry-run)
    #[arg(long)]
    pub confirm: Option<String>,

    /// Solo muestra lo que haría sin tocar la API
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

pub async fn run(config_path: &Path, args: &DeleteDnsArgs) -> Result<(), CoolifyError> {
    let settings = Settings::load(config_path)?;
    let fqdn = if args.name == args.zone || args.name.ends_with(&format!(".{}", args.zone)) {
        args.name.clone()
    } else {
        format!("{}.{}", args.name.trim_end_matches('.'), args.zone)
    };
    if !args.dry_run {
        match args.confirm.as_deref() {
            Some(confirm) if confirm == fqdn => {}
            _ => {
                return Err(CoolifyError::Validation(format!(
                    "Falta --confirm {fqdn} (o usar --dry-run para previsualizar sin borrar)."
                )));
            }
        }
    }

    let report = dns_manager::delete_orphan_dns(
        &settings,
        &args.provider,
        &args.zone,
        &args.name,
        args.ip.as_deref(),
        args.dry_run,
    )
    .await?;

    if args.dry_run {
        println!("[dry-run] Borrado DNS en {} (zona {}):", report.provider, report.zone);
    } else {
        println!("Borrado DNS en {} (zona {}):", report.provider, report.zone);
    }
    for action in &report.actions {
        let symbol = match action.action.as_str() {
            "deleted" | "would-delete" => "  -",
            "kept-remote" => "  !",
            "absent" => "  =",
            _ => "  ?",
        };
        println!(
            "{symbol} {} {} → {} [{action}]",
            action.record_type,
            action.record_name,
            action.value,
            action = action.action
        );
    }
    Ok(())
}
