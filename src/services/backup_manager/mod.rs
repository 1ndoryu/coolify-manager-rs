/* 119A-5 Lote C: split del god-object backup_manager (1468 LE) en modulos <500 LE.
 * Ruta externa intacta: `crate::services::backup_manager::{fns,tipos}` se re-exporta aqui. */

pub mod ayudantes;
pub mod nucleo;
pub mod servidor;
pub mod tipos;

pub use nucleo::{
    create_site_backup, create_site_backup_with_options, list_all_site_backups, list_site_backups,
    materialize_site_backup, prune_site_backup_retention, restore_site_backup,
    upload_site_backup_archive,
};
pub use tipos::{
    BackupArtifact, BackupExecutionOptions, BackupManifest, BackupRemoteMode, BackupStatus,
    DriveBackupEntry, SiteBackupEntries, SiteBackupListFailure, SiteBackupListReport,
};
