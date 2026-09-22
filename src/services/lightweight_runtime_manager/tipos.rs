/* Split 119A-5 de lightweight_runtime_manager.rs — tipos del runtime lightweight.
 * Re-exportados sin cambios desde lightweight_runtime_manager/mod.rs. */

use crate::error::CoolifyError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LightweightSiteInventory {
    pub deployment_id: String,
    pub name: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub project_root: String,
    pub public_root: Option<String>,
    pub containers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightInventoryReport {
    pub target: String,
    pub target_ip: String,
    pub sites: Vec<LightweightSiteInventory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightweightSiteAction {
    Start,
    Stop,
    Restart,
    Reconfigure,
    Delete,
}

impl LightweightSiteAction {
    pub fn parse(raw: &str) -> std::result::Result<Self, CoolifyError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "start" => Ok(Self::Start),
            "stop" => Ok(Self::Stop),
            "restart" => Ok(Self::Restart),
            "reconfigure" => Ok(Self::Reconfigure),
            "delete" => Ok(Self::Delete),
            _ => Err(CoolifyError::Validation(format!(
                "Accion lightweight no soportada '{}'. Usa start, stop, restart, reconfigure o delete.",
                raw
            ))),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reconfigure => "reconfigure",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightSiteActionReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub action: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionStaticSiteReport {
    pub target: String,
    pub target_ip: String,
    pub deployment_id: String,
    pub fqdn: String,
    pub public_url: String,
    pub project_root: String,
    pub public_root: String,
    pub access_user: String,
    pub access_password: String,
    pub access_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupEntry {
    pub backup_id: String,
    pub tier: String,
    pub file_id: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupListReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub entries: Vec<LightweightBackupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightBackupReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub backup_id: String,
    pub tier: String,
    pub status: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightRestoreReport {
    pub target: String,
    pub target_ip: String,
    pub site: String,
    pub backup_id: String,
    pub status: String,
    pub fqdn: Option<String>,
    pub access_user: Option<String>,
    pub access_password: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct RestoreOutputMetadata {
    pub(super) fqdn: Option<String>,
    pub(super) access_user: Option<String>,
    pub(super) access_password: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightweight_action_parse_rejects_unknown_values() {
        assert_eq!(
            LightweightSiteAction::parse("restart").unwrap(),
            LightweightSiteAction::Restart
        );
        assert_eq!(
            LightweightSiteAction::parse("reconfigure").unwrap(),
            LightweightSiteAction::Reconfigure
        );
        assert!(LightweightSiteAction::parse("rotate").is_err());
    }
}
