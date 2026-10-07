/*
 * Cliente SSH nativo usando russh.
 * Reemplaza las llamadas a ssh.exe del PowerShell original.
 * Soporte para ejecucion de comandos remotos, transferencia de archivos y multiplexing.
 */

use crate::config::VpsConfig;
use crate::domain::CommandOutput;
use crate::error::{CoolifyError, SshError};
use crate::infra::ssh_guard;

use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::*;
use std::sync::Arc;

const SSH_TIMEOUT_SECS: u64 = 30;
/* [114A-6] Aumentado de 300s a 1800s (30 min).
 * El build Rust en Docker tarda 10-20 min y puede tener pasos silenciosos >5 min.
 * 300s causaba timeout del canal SSH y el deploy nunca completaba paso [3/6]. */
pub(crate) const CHANNEL_TIMEOUT_SECS: u64 = 1800;

/* [03J-2] CM_GUARD_v1 cableado en execute()/upload (299A-1 07-10):
 * /opt/coolify-guard/ssh-guard.sh verificado (ALLOW / DENY-126 / override reboot).
 * PENDIENTE: standby (auth SSH falla; VPS probablemente reconstruido). */

pub(crate) struct ClientHandler;

/* [259A-2] russh ≥0.52 usa RPITIT nativo: sin #[async_trait]. La firma de
 * check_server_key cambia a &PublicKeyOrCertificate. */
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        /* Aceptar todas las claves del servidor (equivalente al comportamiento de ssh.exe con StrictHostKeyChecking=no) */
        Ok(true)
    }
}

pub struct SshClient {
    pub(crate) host: String,
    user: String,
    ssh_key_path: Option<String>,
    ssh_password: Option<String>,
    pub(crate) session: Option<client::Handle<ClientHandler>>,
    /* [299A-1] true si el host trae el guard ejecutable (sonda en connect()).
     * Hosts sin guard (standby) siguen en directo para no brickearlos.
     * Logica del guard en `super::ssh_guard` (envoltorio + sonda). */
    pub(crate) usa_guard: bool,
}

impl SshClient {
    pub fn new(
        host: &str,
        user: &str,
        ssh_key_path: Option<&str>,
        ssh_password: Option<&str>,
    ) -> Self {
        Self {
            host: host.to_string(),
            user: user.to_string(),
            ssh_key_path: ssh_key_path.map(|s| s.to_string()),
            ssh_password: ssh_password.map(|s| s.to_string()),
            session: None,
            usa_guard: false,
        }
    }

    pub fn from_vps(vps: &VpsConfig) -> Self {
        Self::new(
            &vps.ip,
            &vps.user,
            vps.ssh_key.as_deref(),
            vps.ssh_password.as_deref(),
        )
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    /// Establece conexion SSH al servidor.
    pub async fn connect(&mut self) -> std::result::Result<(), CoolifyError> {
        let config = client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(CHANNEL_TIMEOUT_SECS)),
            /* [185B-3] Keepalive SSH automatico.
             * Previene que firewalls/LB cierren TCP durante builds Rust silenciosos (10-20 min).
             * russh envia keepalive@openssh.com cada 60s; si 3 seguidos fallan, cierra la sesion
             * con Error::KeepaliveTimeout (deteccion limpia en vez de stall silencioso). */
            keepalive_interval: Some(std::time::Duration::from_secs(60)),
            keepalive_max: 3,
            ..Default::default()
        };

        let config = Arc::new(config);
        let handler = ClientHandler;

        let addr = format!("{}:22", self.host);
        let mut session = tokio::time::timeout(
            std::time::Duration::from_secs(SSH_TIMEOUT_SECS),
            client::connect(config, &addr, handler),
        )
        .await
        .map_err(|_| SshError::ChannelTimeout {
            seconds: SSH_TIMEOUT_SECS,
        })?
        .map_err(|e| SshError::ConnectionRefused {
            host: self.host.clone(),
            reason: e.to_string(),
        })?;

        let auth_result = if let Some(password) = self.ssh_password.as_deref() {
            session
                .authenticate_password(&self.user, password)
                .await
                .map_err(|_e| SshError::AuthFailed {
                    user: self.user.clone(),
                    host: self.host.clone(),
                })?
        } else {
            let key_path = self.resolve_key_path();
            /* [259A-2] load_secret_key ahora vive en russh::keys y devuelve
             * PrivateKey (ya no KeyPair); authenticate_publickey exige
             * PrivateKeyWithHashAlg (hash explícito solo para RSA). */
            let key = load_secret_key(&key_path, None).map_err(|_e| SshError::AuthFailed {
                user: self.user.clone(),
                host: self.host.clone(),
            })?;

            session
                .authenticate_publickey(&self.user, PrivateKeyWithHashAlg::new(Arc::new(key), None))
                .await
                .map_err(|_e| SshError::AuthFailed {
                    user: self.user.clone(),
                    host: self.host.clone(),
                })?
        };

        /* [259A-2] authenticate_* devuelven AuthResult, ya no bool. */
        if !matches!(auth_result, client::AuthResult::Success) {
            return Err(SshError::AuthFailed {
                user: self.user.clone(),
                host: self.host.clone(),
            }
            .into());
        }

        /* [299A-1] Sonda del guard ANTES de guardar la sesion (canal directo). */
        self.usa_guard = ssh_guard::sondear(&session).await;
        tracing::debug!(
            "SSH {}@{}: guard {}",
            self.user,
            self.host,
            if self.usa_guard {
                "ACTIVO"
            } else {
                "ausente (directo)"
            }
        );
        self.session = Some(session);
        tracing::debug!("SSH conectado a {}@{}", self.user, self.host);
        Ok(())
    }

    /// Intenta reconectar la sesion SSH. Invalida la sesion actual y crea una nueva.
    async fn ensure_connected(&mut self) -> std::result::Result<(), CoolifyError> {
        self.session = None;
        self.connect().await
    }

    /// Ejecuta un comando remoto y retorna stdout, stderr y exit code.
    pub async fn execute(&self, command: &str) -> std::result::Result<CommandOutput, CoolifyError> {
        let session = self.session.as_ref().ok_or(SshError::Disconnected)?;

        let mut channel =
            session
                .channel_open_session()
                .await
                .map_err(|e| SshError::ConnectionRefused {
                    host: self.host.clone(),
                    reason: e.to_string(),
                })?;

        /* [299A-1] Cada comando pasa por el guard cuando el host lo trae
         * (principal). Hosts sin guard (standby) siguen en directo. */
        let clean_command = command.replace('\r', "");
        let final_command = if self.usa_guard {
            ssh_guard::envolver(&clean_command)
        } else {
            clean_command
        };
        channel
            .exec(true, final_command)
            .await
            .map_err(|e| SshError::CommandFailed {
                exit_code: -1,
                stderr: e.to_string(),
            })?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_code = 0i32;

        loop {
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(CHANNEL_TIMEOUT_SECS),
                channel.wait(),
            )
            .await
            .map_err(|_| SshError::ChannelTimeout {
                seconds: CHANNEL_TIMEOUT_SECS,
            })?;

            match msg {
                Some(ChannelMsg::Data { data }) => {
                    stdout.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                    stderr.extend_from_slice(&data);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    exit_code = exit_status as i32;
                }
                None => break,
                _ => {}
            }
        }

        let stdout = String::from_utf8_lossy(&stdout).to_string();
        let stderr = String::from_utf8_lossy(&stderr).to_string();
        /* [299A-1] El veto del guard (exit 126) se propaga como salida normal
         * (contrato intacto para los llamadores) pero se registra: indica un
         * comando que el servidor bloqueo, no un fallo de red. */
        if self.usa_guard
            && exit_code == ssh_guard::EXIT_VETO
            && stderr.contains(ssh_guard::MARCADOR_VETO)
        {
            tracing::warn!(
                "ssh-guard VETO en {}@{}: {}",
                self.user,
                self.host,
                stderr.trim()
            );
        }
        Ok(CommandOutput {
            stdout,
            stderr,
            exit_code,
        })
    }

    /// Ejecuta un comando de larga duracion usando nohup + polling del log.
    /// Resiste cierres de canal SSH durante builds Rust (10-20 min de silencio).
    /// Devuelve (stdout_combinado, exit_code).
    pub async fn execute_long_running(
        &mut self,
        command: &str,
        log_file: &str,
        poll_interval_secs: u64,
        timeout_secs: u64,
    ) -> std::result::Result<CommandOutput, CoolifyError> {
        /* Lanzar en background con nohup; el log termina con EXIT_CODE:N */
        let launch_cmd = format!(
            "nohup sh -c '{} > {} 2>&1; echo EXIT_CODE:$? >> {}' > /dev/null 2>&1 & echo LAUNCHED",
            command.replace('\'', "'\\''"),
            log_file,
            log_file,
        );
        let launch = self.execute(&launch_cmd).await?;
        if !launch.stdout.contains("LAUNCHED") {
            return Err(CoolifyError::Validation(format!(
                "No se pudo lanzar el proceso en background: {}",
                launch.stdout
            )));
        }

        /* [185B-3] Keepalive SSH: ya configurado en connect() via Config.keepalive_interval.
         * russh envia keepalive@openssh.com automaticamente cada 60s. */

        /* Polling hasta completar o timeout */
        let started = std::time::Instant::now();
        let mut last_heartbeat = 0u64;
        let mut consecutive_failures = 0u32;
        const MAX_CONSECUTIVE_FAILURES: u32 = 5;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(poll_interval_secs)).await;

            let elapsed = started.elapsed().as_secs();

            /* Heartbeat visual cada 120s */
            if elapsed.saturating_sub(last_heartbeat) >= 120 {
                println!("      Proceso largo activo: {elapsed}s transcurridos...");
                last_heartbeat = elapsed;
            }

            /* Timeout absoluto */
            if elapsed >= timeout_secs {
                return Err(CoolifyError::Validation(format!(
                    "Timeout ({timeout_secs}s) esperando build. Ultimo log: {}",
                    /* Intentar leer ultimas lineas del log una vez mas */
                    match self
                        .execute(&format!("tail -5 {} 2>/dev/null", log_file))
                        .await
                    {
                        Ok(out) => out.stdout.trim().to_string(),
                        Err(_) => "(no se pudo leer log)".into(),
                    }
                )));
            }

            /* Intentar verificar el log. Si la sesion SSH murio, reconectar. */
            let check = match self
                .execute(&format!("tail -3 {} 2>/dev/null", log_file))
                .await
            {
                Ok(output) => {
                    consecutive_failures = 0;
                    output
                }
                Err(e) => {
                    consecutive_failures += 1;
                    tracing::warn!(
                        "SSH poll fallo ({consecutive_failures}/{MAX_CONSECUTIVE_FAILURES}): {e}"
                    );

                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        return Err(CoolifyError::Validation(format!(
                            "SSH perdido tras {consecutive_failures} intentos consecutivos durante build. \
                             El proceso remoto puede seguir corriendo. Log: {log_file}\nError: {e}"
                        )));
                    }

                    /* Intentar reconexion */
                    if self.ensure_connected().await.is_err() {
                        continue; /* Fallo la reconexion, reintentar en el proximo ciclo */
                    }

                    /* Sesion reconectada, reintentar check inmediatamente */
                    match self
                        .execute(&format!("tail -3 {} 2>/dev/null", log_file))
                        .await
                    {
                        Ok(output) => {
                            consecutive_failures = 0;
                            output
                        }
                        Err(_) => {
                            consecutive_failures += 1;
                            continue;
                        }
                    }
                }
            };

            if check.stdout.contains("EXIT_CODE:") {
                break;
            }
        }

        /* Leer log completo y exit code */
        let log_content = self
            .execute(&format!("cat {} 2>/dev/null", log_file))
            .await
            .unwrap_or_default();

        let exit_code = log_content
            .stdout
            .lines()
            .rev()
            .find(|l| l.starts_with("EXIT_CODE:"))
            .and_then(|l| l.trim_start_matches("EXIT_CODE:").parse::<i32>().ok())
            .unwrap_or(1);

        /* Limpiar log */
        let _ = self.execute(&format!("rm -f {}", log_file)).await;

        /* [185B-2] Forzar reconexion SSH al terminar el long-running build.
         * Aunque session sea Some(...), el TCP subyacente puede estar muerto despues
         * de ~15 min de build silencioso. Sin esta reconexion, el paso [4/6] falla con
         * "Channel send error" al intentar abrir un nuevo canal en la sesion caduca. */
        let _ = self.ensure_connected().await;

        Ok(CommandOutput {
            stdout: log_content.stdout,
            stderr: String::new(),
            exit_code,
        })
    }

    /// Verifica si la conexion SSH esta activa.
    pub async fn test_connection(&self) -> bool {
        match self.execute("echo ok").await {
            Ok(output) => output.stdout.trim() == "ok",
            Err(_) => false,
        }
    }

    fn resolve_key_path(&self) -> String {
        if let Some(ref key) = self.ssh_key_path {
            return key.clone();
        }
        /* Ruta por defecto de SSH key */
        if let Some(home) = dirs::home_dir() {
            let default_key = home.join(".ssh").join("id_ed25519");
            if default_key.exists() {
                return default_key.display().to_string();
            }
            let rsa_key = home.join(".ssh").join("id_rsa");
            if rsa_key.exists() {
                return rsa_key.display().to_string();
            }
        }
        "~/.ssh/id_ed25519".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::encoding::{base64_decode, base64_encode};

    #[test]
    fn test_base64_roundtrip() {
        let original = b"Hello, World!";
        let encoded = base64_encode(original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_roundtrip_binary() {
        let original: Vec<u8> = (0..=255).collect();
        let encoded = base64_encode(&original);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_base64_empty() {
        let encoded = base64_encode(b"");
        let decoded = base64_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_ssh_client_creation() {
        let client = SshClient::new("1.2.3.4", "root", None, None);
        assert_eq!(client.host, "1.2.3.4");
        assert_eq!(client.user, "root");
        assert!(client.session.is_none());
        /* [299A-1] Sin connect() no hay sonda: el guard nace desactivado
         * (logica y tests del envoltorio en `super::ssh_guard`). */
        assert!(!client.usa_guard);
    }
}
