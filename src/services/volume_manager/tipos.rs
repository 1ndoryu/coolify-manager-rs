/* Tipos internos del sync de compose (119A-5 split volume_manager). */

pub(super) struct ComposeEnvSync {
    pub(super) content: String,
    pub(super) inserted_keys: Vec<String>,
    pub(super) updated_keys: Vec<String>,
}

pub(super) struct ComposeVolumeSync {
    pub(super) content: String,
    pub(super) changed: bool,
}
