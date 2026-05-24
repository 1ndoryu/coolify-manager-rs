# Plan de optimizacion y seguridad portable via coolify-manager-rs

Fecha: 2026-05-24
Target base: standby-vps2
Objetivo: que cualquier mejora de rendimiento, mantenimiento o seguridad viva en coolify-manager-rs y pueda aplicarse igual aunque el sitio o el stack cambie de VPS.

## Estado confirmado tras reboot

- Kernel activo actualizado: `6.8.0-117-generic`
- Control-plane arriba: `coolify`, `coolify-db`, `coolify-redis`, `coolify-realtime`, `coolify-sentinel`
- Sitio validado: `guillermo` con `http_ok=true app_ok=true fatal_logs=false`
- Host post-reboot: `load 3.30 4.00 2.13`, `cpu_some avg10=35.36`, `io_some avg10=1.22`, `io_full avg10=0.58`
- CPU promedio post-reboot: `coolify 58.10%`, `coolify-redis 24.64%`, `coolify-realtime 8.86%`, `php 45.78%`, `dockerd 18.16%`, `soketi-server 14.34%`
- El reboot mejoro el control-plane respecto al estado previo, pero sigue habiendo presion de I/O y carga relevante del panel.

## Principios del plan

- Nada critico debe depender de SSH manual o de una VPS concreta.
- Todo cambio repetible debe exponerse como comando, bandera o politica en coolify-manager-rs.
- Las decisiones deben ser por target y por stack, no hardcodeadas por IP.
- El manager debe poder: medir, aplicar, validar, revertir y reportar.

## Hallazgos externos que cambian el plan

- Ubuntu `unattended-upgrades` soporta `Automatic-Reboot`, pero en instalaciones minimales requiere `update-notifier-common` para que el reboot automatico ocurra de verdad. Tambien puede introducir `RandomSleep` de hasta 30 minutos. Conclusion: si el manager va a ser la fuente de verdad, no conviene mezclar dos automatismos opacos que actualicen o reinicien por su cuenta.
- `systemd` timers permiten `OnCalendar`, `Persistent=true`, zona horaria en la propia expresion y `RandomizedDelaySec`. Conclusion: la forma portable de programar mantenimiento/reboot no es cron ad-hoc, sino units `.service` + `.timer` gestionadas por el manager.
- Docker recomienda `live-restore` para mantener contenedores vivos cuando el daemon se reinicia o se actualiza, pero no sustituye un reboot del host ni cubre cambios mayores del daemon. Conclusion: sirve para reducir impacto de mantenimiento de Docker, no como solucion al problema del reboot del VPS.
- Laravel Horizon expone `minProcesses`, `maxProcesses`, `balanceMaxShift`, `balanceCooldown`, `waits`, `horizon:snapshot` cada 5 minutos y `horizon:terminate` en despliegues. Conclusion: parte del coste del panel puede bajarse ajustando supervisores y observabilidad en vez de reiniciar.
- Redis recomienda desactivar Transparent Huge Pages, revisar `SLOWLOG`, habilitar el monitor de latencia y vigilar swapping, fork y fsync/AOF. Conclusion: el manager debe auditar THP, slowlog, swapping y latencia intrinseca para distinguir panel saturado de nodo ruidoso.

## Decision operativa tomada

- No se adopta reboot diario fijo.
- La politica base pasa a ser `if-required`:
  - reiniciar solo si hay `reboot-required`, drift de kernel/servicios tras mantenimiento o una actualizacion realmente lo exige.
- La politica secundaria pasa a ser `if-drift-detected`:
  - reiniciar en la siguiente ventana solo si durante varias muestras hay degradacion sostenida del host/control-plane y los sitios siguen sanos.
- La ejecucion debe ocurrir solo en horas de baja concurrencia.
- La automatizacion debe vivir en `coolify-manager-rs`, no en cron manual, no en SSH manual y no en timers opacos fuera del manager.

## Politica de ventana de mantenimiento

- El manager debe manejar una ventana diaria de comprobacion, no una ventana diaria de reboot.
- Propuesta inicial:
  - `maintenance_window_start_local=03:00:00`
  - `maintenance_randomized_delay=15m`
  - `maintenance_duration_budget=45m`
  - `Persistent=true` en el timer para no perder la ventana si el host estuvo apagado.
- La zona horaria debe ser explicita por target, con valor IANA real. No usar `America` generico.
- Si no se define zona en un target nuevo, el manager no debe programar mantenimiento automaticamente; debe exigir configuracion explicita o usar una politica `manual-only`.

## Configuracion declarativa propuesta por target

```json
{
  "targets": [
    {
      "name": "standby-vps2",
      "vps": {
        "ip": "173.249.50.44",
        "user": "root",
        "sshKey": null,
        "sshPassword": "..."
      },
      "coolify": {
        "baseUrl": "http://173.249.50.44:8000",
        "apiToken": "...",
        "serverUuid": "...",
        "projectUuid": "...",
        "environmentName": "production"
      },
      "maintenancePolicy": {
        "enabled": true,
        "timezone": "America/Bogota",
        "windowStartLocal": "03:00:00",
        "randomizedDelay": "15m",
        "durationBudget": "45m",
        "rebootPolicy": "if-required",
        "maxRebootFrequency": "weekly",
        "sampleSites": ["guillermo"],
        "driftRules": {
          "requiredConsecutiveSnapshots": 3,
          "avg15GreaterThanCpuCount": true,
          "controlPlaneCpuPercent": 35.0,
          "controlPlaneCpuMultiplierVsBaseline": 2.0,
          "cpuPsiSomeAvg10": 25.0,
          "ioPsiFullAvg10": 1.0
        }
      },
      "securityPolicy": {
        "ssh": {
          "allowRootKeyOnly": true,
          "disablePasswordAuth": true,
          "trustedSourceIps": ["186.14.169.211", "66.94.100.241"]
        },
        "firewall": {
          "enabled": true,
          "allowedTcpPorts": [22, 80, 443]
        }
      },
      "hostProfile": {
        "swapGb": 4,
        "swappiness": 10,
        "vfsCachePressure": 50,
        "dockerLiveRestore": true
      }
    }
  ]
}
```

- `maintenancePolicy`, `securityPolicy` y `hostProfile` deben ser opcionales y heredables desde defaults globales si en el futuro se añade un bloque raiz comun.
- La ausencia de politica nunca debe implicar automatismo silencioso; el comportamiento por defecto debe ser conservador.

## Contrato de comandos nuevos

### `schedule-maintenance`

- Objetivo: instalar o retirar el timer y el service de comprobacion diaria.
- Interfaz propuesta:
  - `schedule-maintenance --target standby-vps2 --dry-run`
  - `schedule-maintenance --target standby-vps2 --apply`
  - `schedule-maintenance --target standby-vps2 --remove`
- Debe:
  - renderizar unit files a partir de la politica del target
  - copiarlos al host
  - `systemctl daemon-reload`
  - `systemctl enable --now ...timer`
  - devolver `systemctl list-timers` filtrado para validar proxima ejecucion

### `check-maintenance-window`

- Objetivo: ejecutarse desde timer o manualmente y decidir `no-op` vs mantenimiento vs reboot.
- Interfaz propuesta:
  - `check-maintenance-window --target standby-vps2`
  - `check-maintenance-window --target standby-vps2 --force-evaluate`
  - `check-maintenance-window --target standby-vps2 --dry-run`
- Debe:
  - recopilar snapshots host/control-plane/health
  - evaluar politicas `if-required` y `if-drift-detected`
  - comprobar bloqueos operativos
  - emitir decision estructurada: `noop`, `maintain-no-reboot`, `maintain-and-reboot`, `blocked`

### `audit-security`

- Objetivo: auditar exposicion y acceso del host de forma portable.
- Interfaz propuesta:
  - `audit-security --target standby-vps2`
  - `audit-security --target standby-vps2 --since 24h`
- Debe incluir:
  - SSH activas/recientes/fallidas
  - `sshd_config` efectivo
  - firewall del host
  - puertos Docker publicados
  - resumen de riesgo y diferencias frente a politica declarada

### `harden-ssh`

- Objetivo: alinear el host con la politica SSH declarada sin perder acceso.
- Interfaz propuesta:
  - `harden-ssh --target standby-vps2 --dry-run`
  - `harden-ssh --target standby-vps2 --apply`
- Debe:
  - validar primero que la clave de recovery funciona
  - escribir override o backup del config previo
  - recargar `sshd` con rollback si falla la validacion posterior

### `audit-redis-latency`

- Objetivo: distinguir Redis lento por carga real vs nodo ruidoso.
- Interfaz propuesta:
  - `audit-redis-latency --target standby-vps2`
  - `audit-redis-latency --target standby-vps2 --watchdog-period 500`
- Debe incluir:
  - `SLOWLOG GET`
  - `LATENCY LATEST`
  - `INFO persistence/stats/memory`
  - THP, `vm.overcommit_memory`, swapping y `latest_fork_usec`

## Matriz de decision operacional

| Estado | Sitios | Host/control-plane | Politica | Accion |
| --- | --- | --- | --- | --- |
| `reboot-required=yes` tras update | Sanos | Irrelevante | `if-required` | reboot en ventana |
| Kernel drift / servicios drift | Sanos | Irrelevante | `if-required` | reboot en ventana |
| No reboot required | Sanos | deriva sostenida > umbral | `if-drift-detected` | reboot en ventana |
| No reboot required | Sanos | deriva leve o muestra aislada | cualquiera | no-op + persistir snapshot |
| Sitios degradados/caidos | No sanos | cualquier estado | cualquiera | incidente; no reboot preventivo automatico |
| Deploy/backup/restore/migrate activos | Sanos o no | cualquier estado | cualquiera | blocked; reintentar siguiente ventana |

## Backlog de implementacion exacto

### Bloque A1

- Extender `DeploymentTargetConfig` con `maintenancePolicy`, `securityPolicy`, `hostProfile`.
- Crear structs serde + defaults conservadores.
- Añadir validacion de zona horaria/campos obligatorios.

### Bloque A2

- Crear `schedule-maintenance`.
- Crear renderer de `.service` y `.timer`.
- Guardar units con nombre estable por target.
- Validar con `systemctl list-timers`.

### Bloque A3

- Crear `check-maintenance-window`.
- Reusar `maintain-host`, `optimize-host`, `audit-control-plane`, `health`.
- Persistir snapshot JSON y decision final.

### Bloque A4

- Crear `audit-security`.
- Añadir chequeo de trusted IPs y puertos expuestos.
- Añadir resumen de riesgo accionable.

### Bloque A5

- Crear `harden-ssh` con rollback defensivo.
- Crear `audit-redis-latency`.
- Añadir chequeos THP/overcommit/live-restore.

## Preguntas ya resueltas por el plan

- ¿Reboot diario? No, salvo mitigacion temporal documentada.
- ¿Chequeo diario? Si.
- ¿Hora fija? Si, ventana de baja concurrencia por target.
- ¿Zona horaria? Explicita y declarativa.
- ¿Decision automatica? Si, pero solo bajo politicas `if-required` o `if-drift-detected`.
- ¿Dependencia de la VPS actual? No; la politica vive en `settings.json` y en el manager.

## Umbrales iniciales para `if-drift-detected`

- El reboot por deriva no debe dispararse por una muestra aislada.
- Requisitos minimos para considerarlo:
  - al menos 3 snapshots consecutivos dentro de la misma ventana o previos a ella
  - los sitios de muestra siguen con `http_ok=true` y `app_ok=true`
  - no hay deploy, restore, backup o migracion activos
- Disparadores iniciales a calibrar por target:
  - `avg15 > cpu_count` de forma sostenida
  - `control_plane_total_cpu > 2x baseline` o `>35%` sostenido
  - degradacion clara frente al baseline post-reboot almacenado por el manager
  - crecimiento sostenido de tiempo de `ScheduledJobManager`, Horizon o Redis sin fallo HTTP aun visible
- El manager debe exigir dos condiciones: deriva del host + salud de sitios todavia verde. Si el sitio ya esta caido, entra flujo de incidente, no reboot preventivo.

## Bloque 1: mantenimiento host-level portable

### 1.1 `maintain-host` completo

- Convertir `maintain-host` en flujo estable con:
  - `--check`: solo detecta paquetes upgradable, held packages, reboot-required y servicios degradados.
  - `--apply`: `apt update/full-upgrade/autoremove` con heartbeat y timeout.
  - `--reboot`: reboot solo si hace falta o si el usuario lo fuerza.
  - `--wait`: esperar a que SSH vuelva, validar kernel nuevo y control-plane.
- Guardar un resumen estructurado: paquetes tocados, kernel previo/nuevo, reboot requerido, reboot exitoso, tiempo total.
- Hacer que funcione sobre cualquier target del `settings.json`, no solo VPS2.

### 1.2 post-checks obligatorios

- Añadir validaciones automáticas post-maintenance:
  - `coolify-control-plane status`
  - `health --name` para una muestra de sitios por target
  - `audit-control-plane --since 15m`
  - `optimize-host --dry-run --samples N`
- El mantenimiento no debe considerarse completo hasta tener estas señales.

### 1.3 ventanas de mantenimiento y politica de reboot

- Añadir al perfil del target:
  - `maintenance_timezone`: zona IANA explicita, por ejemplo `America/Bogota`, `America/Mexico_City` o `America/New_York`
  - `maintenance_window_start_local`: por ejemplo `03:00:00`
  - `maintenance_randomized_delay`: por ejemplo `15m`
  - `reboot_policy`: `never`, `if-required`, `if-drift-detected`, `always-windowed`
  - `reboot_max_frequency`: diario, semanal, manual
- Implementar un comando tipo `schedule-maintenance` que instale timers `systemd` por target usando `.service` + `.timer`, con `Persistent=true` y zona horaria explicita.
- Si se programa una ventana a las `03:00`, debe ser a las `03:00` de la zona del target o de la franja de negocio dominante; `America` no es una zona valida y no conviene hardcodear una sola hora global para todo el continente.
- Recomendacion actual: no usar `always-windowed` como politica por defecto. El reboot diario puede ocultar leaks, colas lentas o degradacion del nodo. Debe existir como modo de mitigacion, no como primer arreglo.
- Politica recomendada por defecto:
  - `if-required`: reboot solo si hay `reboot-required` o drift de kernel/servicios tras mantenimiento
  - `if-drift-detected`: reboot en la siguiente ventana si durante N muestras consecutivas el host supera umbrales de load/PSI/control-plane y los sitios siguen sanos
- Decisión actual del proyecto:
  - activar `if-required` como default
  - dejar `if-drift-detected` disponible pero sujeto a umbrales y baseline por target
  - no habilitar `always-windowed` / reboot diario fijo salvo mitigacion temporal documentada
- Si aun asi se quiere reboot diario por mitigacion operativa, debe hacerse con guardas:
  - no desplegar, no backup, no restore, no migracion en curso
  - control-plane arriba y sitios sanos antes de dormir la ventana
  - espera a vuelta de SSH
  - validacion post-reboot de kernel, control-plane y health de muestra
  - alerta si falla la vuelta del target

### 1.4 fuente unica de verdad para updates y reboots

- Si `coolify-manager-rs` asume mantenimiento programado, el manager debe auditar y opcionalmente neutralizar solapes con:
  - `APT::Periodic::*`
  - `unattended-upgrades`
  - cron legacy de mantenimiento
  - timers systemd preexistentes
- Objetivo: evitar que `apt` automatico y el manager compitan por el lock de `dpkg` o reinicien fuera de ventana.

### 1.5 flujo objetivo del timer diario

- El timer diario del manager debe ejecutar un `check-maintenance-window` o equivalente con esta secuencia:
  - recopilar snapshot host/control-plane
  - evaluar politica `if-required`
  - evaluar politica `if-drift-detected`
  - comprobar bloqueo operativo: deploy/backup/restore/migrate en curso
  - comprobar salud de sitios de muestra
  - si no se cumple criterio, terminar sin reboot y persistir snapshot
  - si se cumple criterio, ejecutar reboot controlado con `--wait` y post-checks
- Resultado esperado:
  - el sistema revisa cada noche a baja concurrencia
  - solo reinicia cuando realmente hay una razon operativa suficiente
  - cada no-op y cada reboot quedan auditados por el manager

## Bloque 2: optimizacion del control-plane

### 2.1 auditoria y reparacion de Horizon/Scheduler

- Extender `audit-control-plane` para extraer:
  - frecuencia real de `ScheduledJobManager`
  - jobs lentos por percentiles
  - solapamiento de scheduler/horizon
  - numero de workers activos y colas observadas
- Extender `audit-control-plane --repair` con reparaciones seguras:
  - reinicio ordenado de Horizon
  - limpieza de estado efimero de colas si no hay jobs utiles
  - validacion de recovery post-repair

### 2.2 presupuesto de CPU del panel

- Crear un comando nuevo tipo `tune-control-plane` para imponer perfiles conservadores por target:
  - reducir concurrencia de workers del panel
  - revisar/reducir tareas programadas de alta frecuencia
  - limitar consumo de `soketi` y `coolify-realtime` cuando el target no necesita tanta actividad realtime
- Incluir parametros inspirados en Horizon:
  - `minProcesses`, `maxProcesses`
  - `balanceMaxShift`, `balanceCooldown`
  - umbrales `waits`
  - validacion de `horizon:snapshot` cada 5 minutos y no mas frecuentemente
- Exponer un perfil "conservador" y uno "agresivo" por target para no tocar a ciegas el panel.
- El perfil debe vivir como configuracion del target, no como cambio manual dentro de contenedores.

### 2.3 baseline por target

- Guardar en el manager una “foto buena” por target:
  - load aceptable
  - CPU del control-plane aceptable
  - presión PSI aceptable
  - latencia HTTP esperada
- Luego `audit-control-plane` y `optimize-host` comparan contra ese baseline y no solo contra valores absolutos.

## Bloque 3: optimizacion de I/O y aislamiento

### 3.1 storage benchmark repetible

- Ampliar `audit`/`optimize-host` con benchmarks host-level acotados:
  - escritura secuencial con `fdatasync`
  - latencia de fsync
  - espacio libre por filesystem
  - inodos libres
- Guardar historial corto por target para detectar degradacion del nodo proveedor.

### 3.1.b Docker daemon y tiempo de mantenimiento

- Añadir auditoria/configuracion de `live-restore` en Docker para reducir impacto cuando solo se reinicia o recarga `dockerd`.
- No tratar `live-restore` como sustituto de reboot del host: debe quedar documentado en el manager que ayuda en upgrades/reloads del daemon, pero no corrige leaks del kernel ni evita reinicios completos.

### 3.2 identificar workloads ruidosos

- Extender `optimize-host` para clasificar consumo por grupos:
  - panel Coolify
  - WordPress/PHP
  - Rust apps
  - base de datos
  - realtime/websocket
- Objetivo: poder decidir si una degradacion viene del panel, del sitio o del proveedor.

### 3.2.b Redis como factor de latencia

- Añadir un `audit-redis-latency` o sub-bloque dentro de `audit-control-plane` para inspeccionar:
  - `SLOWLOG GET`
  - `LATENCY LATEST` / latencia monitor cuando aplique
  - `latest_fork_usec`
  - swapping del proceso Redis
  - THP habilitado/deshabilitado
  - `vm.overcommit_memory`
- Si se confirma Redis como cuello de botella, crear `tune-redis-host` para:
  - desactivar THP de forma persistente
  - fijar `vm.overcommit_memory=1` cuando corresponda
  - revisar persistencia AOF/fsync solo si el rol de Redis lo permite
  - evitar barridos peligrosos tipo `KEYS` en diagnosticos del propio manager

### 3.3 politicas de aislamiento

- Añadir en el manager recomendaciones automáticas cuando se superen umbrales:
  - mover control-plane a VPS dedicada
  - separar DB pesada de panel
  - migrar sitios ruidosos a otro target
  - preferir targets distintos para workloads realtime o builders pesados
- Estas decisiones deben poder ejecutarse con comandos existentes o nuevos (`migrate`, `failover`, `switch-dns`, perfiles de target).

### 3.4 optimizaciones adicionales a evaluar

- `dockerd`:
  - auditar y opcionalmente habilitar `live-restore`
  - revisar logging driver y rotacion para evitar presion de disco por logs de contenedores
- `systemd`:
  - identificar servicios que arrastran consumo post-boot o se reactivan innecesariamente
  - medir tiempos de arranque tras reboot para saber si el mantenimiento cabe en la ventana
- `PHP/WordPress`:
  - medir si el ruido viene de `php-fpm`/cron interno o del panel
  - empujar `disable_wp_cron` y cron del sistema donde aplique
- `Docker build/runtime`:
  - separar builders pesados o caches de build del mismo host de produccion cuando el target comparta panel y sitios

## Bloque 4: seguridad host y acceso

### 4.1 auditoria SSH portable

- Consolidar en `optimize-host` y/o nuevo `audit-security`:
  - sesiones activas por IP
  - `Accepted` recientes por IP
  - `Failed password` y `Invalid user` recientes
  - deteccion de IPs externas no conocidas
  - resumen de root login, password auth y allowlists
- Permitir un inventario de IPs confiables por target para alertar sobre IPs no reconocidas.

### 4.2 endurecimiento SSH por manager

- Nuevo comando `harden-ssh` con modo `--dry-run` y `--apply`:
  - desactivar password auth cuando haya clave verificada
  - desactivar root login por password
  - opcionalmente permitir root solo por clave o crear usuario admin alterno
  - aplicar `AllowUsers` o allowlist por IP/segmento cuando el target lo soporte
  - validar que la clave de recovery siga funcionando antes de cerrar la sesion

### 4.3 firewall y superficie expuesta

- Nuevo comando `audit-firewall`:
  - puertos expuestos por host
  - puertos publicados por Docker
  - diferencias entre lo esperado y lo publicado
- Nuevo comando `tune-firewall`:
  - cerrar puertos no usados
  - dejar abiertos solo `22`, `80`, `443` y lo estrictamente necesario
  - perfilar excepciones por target y por stack

### 4.4 secretos y credenciales

- Añadir un flujo de `rotate-secrets` por target/sitio para:
  - credenciales SSH montadas en runtime
  - variables de Coolify sensibles
  - passwords DB cuando aplique
  - claves de aplicaciones auxiliares
- El manager debe verificar dependencias antes de rotar y testear health después.

## Bloque 5: observabilidad y alertas

### 5.1 alertas de salud del host

- Añadir alertas para:
  - `reboot-required` demasiado tiempo
  - PSI CPU/I/O por encima de umbral
  - load promedio sostenido
  - control-plane por encima de presupuesto CPU
  - fallos repetidos de `ScheduledJobManager`

### 5.2 snapshots comparables

- Guardar snapshots JSON por target desde el manager:
  - host
  - control-plane
  - health de sitios
  - seguridad SSH/firewall
- Esto permite comparar VPS vieja vs nueva antes de migrar o durante failover.

### 5.3 alertas de deriva acumulativa

- Como el reboot bajo claramente el load, añadir alertas para detectar el patron de "degradacion acumulativa":
  - subida sostenida del control-plane frente al baseline post-reboot
  - empeoramiento del avg15 sin cambios de trafico equivalentes
  - crecimiento continuo de tiempo de `ScheduledJobManager`
  - crecimiento de cola/latencia Redis sin fallos HTTP todavia visibles
- Objetivo: reiniciar por condicion antes de caer en estado malo, no despues.

## Bloque 6: migracion y portabilidad entre VPS

### 6.1 perfil declarativo por target

- Mover a configuracion declarativa del manager:
  - swap esperada
  - sysctl perfilado
  - politica de SSH
  - politica de firewall
  - presupuesto del control-plane
  - ventana de mantenimiento y politica de reboot
  - checks post-reboot
- Un target nuevo debe poder bootstrapearse a partir de ese perfil sin pasos manuales ocultos.

### 6.2 comando de bootstrap integral

- Evolucionar `install-coolify`/`maintain-host` hacia un `bootstrap-target` o similar que haga:
  - update inicial
  - hardening SSH
  - swap/sysctl
  - firewall
  - install/repair de Coolify
  - baseline inicial de auditoria
  - validacion final

## Prioridad recomendada

### Fase A: inmediata

- Terminar `maintain-host` con `--wait` y post-checks.
- Corregir el comportamiento de dry-run para no dar falsos pendientes.
- Añadir `audit-security` con SSH activas, recientes y fallos.
- Añadir deteccion de `reboot-required` envejecido y alertas.
- Añadir `schedule-maintenance` con politica `if-required` y ventana por zona horaria.
- Añadir `check-maintenance-window` que decida no-op vs reboot dentro de la ventana de baja concurrencia.

### Fase B: alto impacto

- Extender `audit-control-plane` con scheduler/horizon detallado.
- Crear `tune-control-plane` por target.
- Crear `audit-firewall` y `harden-ssh`.
- Añadir snapshots JSON comparables por target.
- Añadir `audit-redis-latency` y chequeo de THP/overcommit.
- Añadir soporte de `live-restore` en auditoria/configuracion Docker.
- Añadir baseline post-reboot por target y reglas de deriva para `if-drift-detected`.

### Fase C: estructural

- Declarar perfiles completos por target.
- Crear `bootstrap-target` integral.
- Automatizar recomendaciones de migracion/aislamiento entre VPS.
- Añadir politica de reboot condicional por deriva y no solo por paquetes pendientes.

## Riesgos a vigilar

- El reboot arreglo parte del problema, pero no elimino la presion de I/O del nodo.
- `coolify` sigue siendo el contenedor con mas CPU promedio post-reboot.
- Aparecen IPs SSH externas activas/recientes que deben clasificarse como tuyas o no confiables.
- Algunas optimizaciones del panel pueden depender de la version interna de Coolify/Laravel; deben aplicarse con deteccion de capacidades, no a ciegas.
- Un reboot diario puede estabilizar sintomas, pero tambien puede esconder la causa raiz y normalizar deuda operativa si no se acompaña de alertas y tuning real.
- Si conviven `unattended-upgrades` y mantenimiento del manager, pueden reaparecer locks de `dpkg` o reboots fuera de ventana.

## Criterio de exito

- Un target nuevo puede quedar endurecido, mantenido y auditado desde coolify-manager-rs sin SSH manual.
- El control-plane no supera un presupuesto razonable de CPU en reposo.
- El host mantiene PSI I/O y CPU dentro de umbrales definidos.
- Las alertas distinguen entre fallo del sitio, fallo del panel y degradacion del nodo.
- El cambio de VPS no rompe el flujo porque las politicas viven en el manager y en el perfil del target.
- Si se habilita reboot programado, este se ejecuta solo en ventana, con zona horaria explicita, validacion previa y chequeo post-reboot automatizado.
- El timer diario puede ejecutarse todos los dias, pero el reboot solo ocurre cuando la politica `if-required` o `if-drift-detected` lo justifica.