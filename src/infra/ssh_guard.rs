/* ssh-guard server-side (CM_GUARD_v1, 299A-1): wrapper que veta comandos
 * destructivos antes de ejecutarlos (`config/guard/ssh-guard.sh`, instalado en
 * el VPS principal `/opt/coolify-guard/ssh-guard.sh`). Recibe el comando como
 * argv unico; su veto sale con exit 126 y marcador en stderr. */

use russh::client;
use russh::ChannelMsg;

/* Ruta canonica del guard en los hosts gestionados. */
pub const RUTA_GUARD: &str = "/opt/coolify-guard/ssh-guard.sh";
/* Exit con el que el guard senala un veto (nada se ejecuto). */
pub const EXIT_VETO: i32 = 126;
/* Marcador en stderr que identifica un veto del guard. */
pub const MARCADOR_VETO: &str = "ssh-guard: comando vetado";

/// Envuelve un comando para pasar por el guard (escapando comillas simples).
pub fn envolver(comando: &str) -> String {
    format!("{RUTA_GUARD} '{}'", comando.replace('\'', "'\\''"))
}

/// Sonda server-side (canal directo, sin envolver): true si el guard existe y
/// es ejecutable. Best-effort con timeout corto; ante la duda, false (el veto
/// real vive en el servidor, la sonda solo decide envolver o no).
pub async fn sondear<H: client::Handler>(session: &client::Handle<H>) -> bool {
    let mut channel = match session.channel_open_session().await {
        Ok(c) => c,
        Err(_) => return false,
    };
    if channel
        .exec(true, format!("test -x {RUTA_GUARD} && echo GUARD_OK"))
        .await
        .is_err()
    {
        return false;
    }
    let espera = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data })
                    if String::from_utf8_lossy(&data).contains("GUARD_OK") =>
                {
                    return true;
                }
                None => return false,
                _ => {}
            }
        }
    })
    .await;
    matches!(espera, Ok(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envolver_simple() {
        assert_eq!(
            envolver("echo ok"),
            "/opt/coolify-guard/ssh-guard.sh 'echo ok'"
        );
    }

    #[test]
    fn test_envolver_escapa_comillas() {
        assert_eq!(
            envolver("echo 'hola'"),
            "/opt/coolify-guard/ssh-guard.sh 'echo '\\''hola'\\'''"
        );
    }

    #[test]
    fn test_envolver_vetado_pasa_intacto() {
        /* El envoltorio NO filtra: el veto lo decide el servidor (exit 126). */
        let cmd = "rm -rf /";
        assert!(envolver(cmd).ends_with(&format!("'{cmd}'")));
    }
}
