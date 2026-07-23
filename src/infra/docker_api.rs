/*
 * Docker Engine API client — alternativa a SSH para operaciones de logs/exec.
 * Usa bollard para conectar al Docker daemon via Unix socket o TCP.
 *
 * Uso típico: `cm logs --name studio --target mariadb --docker-socket tcp://66.94.100.241:2375`
 * Requiere que el Docker daemon acepte conexiones en el endpoint indicado.
 */

use crate::error::CoolifyError;
use bollard::container::LogsOptions;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;

/// Cliente para el Docker Engine API.
pub struct DockerApiClient {
    docker: Docker,
}

impl DockerApiClient {
    /// Conecta al Docker daemon.
    ///
    /// `endpoint` puede ser:
    /// - `unix:///var/run/docker.sock` (local, por defecto)
    /// - `tcp://host:2375` (remoto sin TLS)
    /// - `npipe:////./pipe/docker_engine` (Windows)
    /// - `None` → usa el default de bollard (unix socket local)
    pub fn connect(endpoint: Option<&str>) -> std::result::Result<Self, CoolifyError> {
        let docker = match endpoint {
            Some(ep) if ep.starts_with("tcp://") => {
                let host = ep.strip_prefix("tcp://").unwrap_or(ep);
                Docker::connect_with_http(host, 6, bollard::API_DEFAULT_VERSION)
                    .map_err(|e| CoolifyError::DockerApi(format!("conexion TCP a {ep}: {e}")))?
            }
            Some(ep) if ep.starts_with("unix://") || ep.starts_with("/") => {
                Docker::connect_with_socket(ep, 6, bollard::API_DEFAULT_VERSION)
                    .map_err(|e| CoolifyError::DockerApi(format!("conexion socket a {ep}: {e}")))?
            }
            Some(ep) if ep.starts_with("npipe://") => {
                Docker::connect_with_named_pipe(ep, 6, bollard::API_DEFAULT_VERSION).map_err(
                    |e| CoolifyError::DockerApi(format!("conexion npipe a {ep}: {e}")),
                )?
            }
            Some(ep) => {
                return Err(CoolifyError::DockerApi(format!(
                    "endpoint no reconocido: {ep}. Usa tcp://, unix://, npipe:// o ruta directa"
                )));
            }
            None => Docker::connect_with_local_defaults().map_err(|e| {
                CoolifyError::DockerApi(format!("conexion local default: {e}"))
            })?,
        };

        Ok(Self { docker })
    }

    /// Verifica que el daemon Docker responde.
    pub async fn ping(&self) -> std::result::Result<(), CoolifyError> {
        self.docker
            .ping()
            .await
            .map_err(|e| CoolifyError::DockerApi(format!("ping fallo: {e}")))?;
        Ok(())
    }

    /// Lista contenedores cuyo nombre contiene `name_fragment`.
    pub async fn find_containers(
        &self,
        name_fragment: &str,
    ) -> std::result::Result<Vec<ContainerSummary>, CoolifyError> {
        let mut filters = HashMap::new();
        filters.insert("name", vec![name_fragment]);

        let options = Some(bollard::container::ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        });

        let containers = self
            .docker
            .list_containers(options)
            .await
            .map_err(|e| CoolifyError::DockerApi(format!("listando contenedores: {e}")))?;

        Ok(containers
            .into_iter()
            .map(|c| ContainerSummary {
                id: c.id.unwrap_or_default(),
                names: c.names.unwrap_or_default(),
                image: c.image.unwrap_or_default(),
                state: c.state.unwrap_or_default(),
                status: c.status.unwrap_or_default(),
            })
            .collect())
    }

    /// Obtiene logs de un contenedor por ID o nombre.
    ///
    /// `tail`: número de líneas finales (0 = todas).
    /// `since`: segs desde epoch para filtrar (0 = sin filtro).
    /// `stdout`/`stderr`: qué streams incluir.
    pub async fn container_logs(
        &self,
        container: &str,
        tail: u32,
        since: i64,
        stdout: bool,
        stderr: bool,
    ) -> std::result::Result<LogOutput, CoolifyError> {
        let tail_str = if tail == 0 {
            "all".to_string()
        } else {
            tail.to_string()
        };

        let options = LogsOptions::<String> {
            tail: tail_str,
            stdout,
            stderr,
            timestamps: true,
            since,
            ..Default::default()
        };

        let mut stream = self.docker.logs(container, Some(options));

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(log) => match log {
                    bollard::container::LogOutput::StdOut { message } => {
                        stdout_buf.push_str(&String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr_buf.push_str(&String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::Console { message } => {
                        stdout_buf.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                },
                Err(e) => {
                    return Err(CoolifyError::DockerApi(format!(
                        "leyendo logs de {container}: {e}"
                    )));
                }
            }
        }

        Ok(LogOutput {
            stdout: stdout_buf,
            stderr: stderr_buf,
        })
    }

    /// Obtiene el nombre del primer contenedor que coincide con un fragmento de nombre.
    /// Útil para resolver `--target mariadb` → nombre real del contenedor.
    pub async fn resolve_container_name(
        &self,
        name_fragment: &str,
    ) -> std::result::Result<String, CoolifyError> {
        let containers = self.find_containers(name_fragment).await?;

        if containers.is_empty() {
            return Err(CoolifyError::DockerApi(format!(
                "no se encontro contenedor con nombre '{name_fragment}'"
            )));
        }

        // Preferir el contenedor que mejor coincide (nombre más corto = más específico)
        let best = containers
            .iter()
            .min_by_key(|c| c.names.first().map(|n| n.len()).unwrap_or(usize::MAX))
            .unwrap();

        Ok(best
            .names
            .first()
            .cloned()
            .unwrap_or_else(|| best.id.clone()))
    }
}

#[derive(Debug)]
pub struct ContainerSummary {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub state: String,
    pub status: String,
}

#[derive(Debug)]
pub struct LogOutput {
    pub stdout: String,
    pub stderr: String,
}
