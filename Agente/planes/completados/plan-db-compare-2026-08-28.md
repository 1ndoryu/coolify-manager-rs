# Plan E12 — `db-compare`: comparación automática y precisa de bases de datos

> Fecha: 2026-08-28 · Proyecto: coolify-manager-rs · Estado: **COMPLETADO (implementado y verificado en producción)**
> Motivación: el método manual de comparación (grep/awk/zcat sobre dumps SQL) resultó impreciso,
> frágil e inaplicable a proyectos con **tablas personalizadas desconocidas** (pgvector, plugins WP,
> tipos custom). Este plan propone automatizar la comparación DENTRO de la BD (SQL real), sin parsear
> texto y sin hardcodear listas de tablas.
>
> **Nota de implementación (28/08):** la comparación precisa usa **JSON canónico por fila + conjuntos
> en Rust** (no `EXCEPT` entre contenedores, porque dos contenedores distintos no pueden referenciarse
> entre sí). Ambos lados se extraen con `SELECT to_json/JSON_OBJECT` y se comparan como conjuntos en
> memoria (correcto, determinista y sin parsear el dump como texto). Ver sección §3.3.

---

## 1. Objetivo

Añadir al CLI un comando `db-compare` que compare **de forma segura, precisa y reproducible** dos
versiones de una base de datos:

- **BD en vivo** de un sitio (PostgreSQL o MariaDB) vs **un dump** (`.sql.gz` del VPS, o `.sql` local).
- **BD en vivo de un sitio** vs **BD en vivo de otro sitio** (mismo tipo de motor).
- Opcionalmente, solo **dos tablas** concretas (`--tables t1,t2`) en lugar de toda la BD.

Requisitos que debe cumplir (deducidos del incidente y de las críticas del usuario):

1. **No parsear dumps SQL** con grep/awk/cuentas de `(`. La comparación se hace con SQL real.
2. **Descubrir tablas automáticamente** (`information_schema` / `SHOW TABLES`). Nunca hardcodear la
   lista: cada proyecto tiene tablas custom que el manager no conoce.
3. **Solo lectura** contra la BD en vivo. No modifica, no inserta, no restaura nada.
4. **Salida estructurada** (JSON estable) con: tablas solo-en-A, solo-en-B, idénticas, con diferencia,
   y muestra limitada de filas distintas.
5. **Manejar casos especiales** sin fallar: columnas `vector` (pgvector), tablas sin PK, `bytea`/text
   grande, columnas volátiles (timestamps), spam/comentarios no aprobados.
6. **Reproducible**: mismo comando → mismo resultado; versión de motor documentada en el reporte.

---

## 2. Por qué NO seguir con el método manual (registro de la decisión)

| Problema observado | Consecuencia | Cómo lo resuelve `db-compare` |
|---|---|---|
| INSERT multi-fila con contenido enorme en dumps | conteos de `(` falsos, líneas truncadas | No lee el dump como texto; lo restaura y consulta con SQL |
| Tablas personalizadas desconocidas | grep no sabe qué buscar | Descubrimiento automático de esquema |
| pgvector (`$libdir/vector`) | consultas a tablas con vector fallan | Detecta columnas `vector` y las excluye/degrada con alerta |
| `comm`/`sort` sobre texto | orden inestable, duplicados, warnings | `EXCEPT`/`FULL OUTER JOIN` nativos del motor |
| Spam y contenido no aprobado | ruido que confunde el diff | Filtros declarativos por estado (p. ej. `post_status`) y columnas ignorables |

---

## 3. Arquitectura propuesta (anclada en código existente)

### 3.1 Nuevos archivos

```
src/commands/db_compare.rs            — entry point del comando (patrón db_check/run_sql)
src/services/compare_manager.rs       — orquesta el flujo completo
src/services/compare/
  mod.rs
  schema.rs                           — descubrimiento de esquema (PG + MariaDB)
  digest.rs                           — hash canónico por tabla (para triage rápido)
  diff.rs                             — diff preciso por tabla (EXCEPT / FULL OUTER JOIN / NOT IN)
  report.rs                           — modelo de salida JSON
src/infra/db_tmp.rs                   — contenedor temporal efímero (restauración aislada)
```

### 3.2 Registro

- `src/cli/mod.rs`: variante `DbCompare { ... }` (junto a `DbCheck`, `DbStats`).
- `src/cli/dispatch/ops.rs`: enrutar `Command::DbCompare` → `commands::db_compare::execute`.
- `src/mcp/tools.rs` (si aplica): exponer como tool MCP con JSON estable.

### 3.3 Firmas del comando

```
coolify-manager db-compare --name <sitio>
    [--dump <ruta-local|ruta-vps>]      # contra un dump (default: último dump VPS del sitio)
    [--against <otro-sitio>]            # o contra otro sitio en vivo
    [--tables t1,t2]                    # limitar a tablas concretas (opcional)
    [--ignore-columns c1,c2]            # columnas volátiles a excluir de la comparación
    [--limit-diff N]                    # máx. filas de muestra por tabla (default 20)
    [--json]                            # salida JSON estable (default: texto formateado)
    [--no-tmp-container]                # modo ligero: solo conteos + hashes sin contenedor temporal
```

---

## 4. Flujo del comando (paso a paso)

### Fase 0 — Preflight (todo igual a `run_sql`/`db_check`)

1. `Settings::load` → `get_site` → `validation::assert_site_ready` → `resolve_site_target`.
2. `SshClient::from_vps` + `connect`.
3. Resolver motor y credenciales:
   - **PostgreSQL**: `pg_utils::get_pg_credentials` → `(pg_container, db_user, db_name, url)`.
   - **MariaDB/WordPress**: `database_manager::resolve_wordpress_credentials` → `(db_name, db_user, db_password)`.
4. Localizar el dump:
   - `--dump <path>`: path local (se sube si falta en VPS) o path VPS.
   - sin `--dump` y sin `--against`: listar `/data/backups/<stack_uuid>/daily/` (y `weekly/`) en el
     VPS y elegir el `.sql.gz` más reciente **que no sea el que se está generando ahora**.
   - `--against <otro-sitio>`: segundo `SshClient` al target del otro sitio (mismo flujo de credenciales).

### Fase 1 — Descubrimiento de esquema (AUTOMÁTICO, sin hardcodear)

**PostgreSQL** (`run_pg_query` con `psql -t -A`):

```sql
SELECT table_name FROM information_schema.tables
WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
ORDER BY table_name;
```

Por tabla, detectar columnas y tipos:

```sql
SELECT column_name, data_type
FROM information_schema.columns
WHERE table_schema = 'public' AND table_name = '<t>'
ORDER BY ordinal_position;
```

Detectar columnas **vector** (pgvector) vía catálogo:

```sql
SELECT c.relname, a.attname
FROM pg_class c
JOIN pg_attribute a ON a.attrelid = c.oid
JOIN pg_type t ON t.oid = a.atttypid
WHERE c.relname IN (<tablas>) AND t.typname = 'vector';
```

Detectar columnas de tipo bytea / sin PK:

```sql
-- sin PK
SELECT c.relname
FROM pg_class c
LEFT JOIN pg_constraint con ON con.conrelid = c.oid AND con.contype = 'p'
WHERE c.relkind = 'r' AND c.relname IN (<tablas>) AND con.oid IS NULL;
```

**MariaDB** (vía `docker exec <mariadb> mariadb -u <u> -p<pass> <db> -N -e`):

```sql
SHOW TABLES;
SHOW COLUMNS FROM <t>;
SHOW INDEX FROM <t> WHERE Key_name = 'PRIMARY';
```

Resultado: `SchemaModel { engine, tables: { name → { columns, pk, has_vector, has_bytea } } }`
para **ambos lados** (vivo y dump/otro sitio). Las tablas solo-en-un-lado se reportan, no se ignoran.

### Fase 2 — Modo ligero `--no-tmp-container` (rápido, sin levantar nada)

Cuando el usuario solo quiere un triage rápido (sin el contenedor temporal):

- `SELECT COUNT(*)` por tabla en ambos lados.
- **Hash canónico por tabla** (mismo SQL en ambos lados), p. ej. para PG:

```sql
SELECT md5(string_agg(r::text, E'\n' ORDER BY r))
FROM (
  SELECT row_to_json(t)::text AS r
  FROM <tabla> t
  ORDER BY 1
) s;
```

> Nota de diseño: `row_to_json` falla con `bytea` y `vector`; para esas tablas el digest usa
> `encode(col::bytea,'hex')` por columna no especial, y **marca la tabla como "no comparable en
> modo ligero"** (se recomienda el modo con contenedor temporal). Este modo da certeza sobre
> **conteos** y **sospecha de igualdad**, no sobre filas exactas.

### Fase 3 — Modo completo: contenedor temporal (la comparación PRECISA)

Este es el núcleo del plan: **en lugar de parsear el dump, se restaura en un contenedor efímero
aislado y se comparan las dos BDs con SQL nativo.**

1. **Preparar el dump**:
   - Si es `.sql.gz` del VPS: descargarlo localmente (`ssh` → `scp`/lectura) o dejarlo en el VPS.
   - Si es `.sql` local: `upload_file` al VPS (patrón de `import_database.rs`).
2. **Levantar contenedor temporal** en el VPS (`docker run --rm -d`), con:
   - **Misma imagen** que el motor del stack (p. ej. la imagen del postgres/mariadb del sitio) para
     máxima compatibilidad de formatos y extensiones.
   - Nombre único con sufijo aleatorio (`coolify-dbcompare-<hash>`), **red aislada**, sin puertos
     publicados, `--restart no`.
   - Si el dump necesita pgvector: usar la imagen del stack (que puede no tener la extensión) o
     intentar `CREATE EXTENSION` dentro del contenedor temporal; si falla, degradar esas tablas
     con alerta explícita (nunca abortar todo).
3. **Restaurar** el dump dentro del contenedor temporal:
   - PG: `gunzip -c dump.sql.gz | docker exec -i <tmp> psql -U <user> -d <db>` (o `pg_restore` si
     es dump binario — detectar por cabecera).
   - MariaDB: `gunzip -c dump.sql.gz | docker exec -i <tmp> mariadb -u <user> <db>` (patrón de
     `database_manager::import_database` pero contra el contenedor TEMPORAL, no el vivo).
4. **Comparar dentro de SQL**: con ambos lados disponibles como BDs reales, el diff es nativo y
   preciso:
   - **Conteos**: `COUNT(*)` en cada lado por tabla.
   - **Filas solo-en-A / solo-en-B / iguales**:
     - PostgreSQL: `SELECT * FROM vivo.t EXCEPT SELECT * FROM tmp.t` y el inverso. (`EXCEPT` es
       nativo, orden-independiente y tipado.)
     - MariaDB (10.3+): `EXCEPT` soportado; fallback `LEFT JOIN` + `IS NULL` o `NOT IN` con clave.
   - **Cuantificar y muestrear**: `SELECT COUNT(*) FROM (<EXCEPT>) x` + `LIMIT <N>` para la muestra.
   - **Tablas con columnas `vector`**: comparar la tabla ignorando la columna vector (proyección de
     columnas no-vector) y marcar `"vector_column_ignored": true` en el reporte.
   - **Tablas sin PK**: igualmente comparables con `EXCEPT` (no requiere PK); la muestra se ordena
     por la proyección canónica.
   - **Columnas `bytea`**: `EXCEPT` las maneja nativamente (compara bytes), sin parsing.
5. **Garantía de limpieza**: `docker rm -f` del contenedor temporal en `finally`/`Drop`, con un
   `timeout` total (p. ej. 10 min) y registro de que se limpió. Nunca deja contenedores huérfanos.

> **Alternativa descartada (documentada):** importar el dump en un *esquema temporal* de la BD viva
> (`CREATE SCHEMA tmp`). Rechazada porque escribe en la BD de producción y exige privilegios
> elevados; el contenedor efímero no toca la BD viva en absoluto.

### Fase 4 — Reporte

Salida JSON estable (con `--json`) con esta forma:

```json
{
  "sitio": "studio",
  "motor": "postgres",
  "dump": "/data/backups/postgres-do8k.../daily/studio_20260827_010000.sql.gz",
  "dump_restaurado": true,
  "motor_dump": "PostgreSQL 16.x",
  "fecha_verificacion": "2026-08-28T...",
  "resumen": {
    "tablas_vivo": 12,
    "tablas_dump": 12,
    "tablas_solo_vivo": 0,
    "tablas_solo_dump": 0,
    "tablas_identicas": 9,
    "tablas_con_diferencia": 3,
    "tablas_no_comparables": 1
  },
  "tablas": {
    "hosting_subscriptions": {
      "estado": "con_diferencia",
      "filas_vivo": 14,
      "filas_dump": 14,
      "diferencias": 2,
      "solo_en_vivo": [{ "id": 7, "...": "..." }],
      "solo_en_dump": [{ "id": 99, "...": "..." }],
      "vector_column_ignored": false
    },
    "infra_samples": { "estado": "solo_en_vivo", "filas_vivo": 8954, "filas_dump": 0 }
  }
}
```

En modo texto, el mismo contenido formateado con tablas de resumen y columnas de detalle.

### Fase 5 — Seguridad (invariantes)

1. **Solo lectura** en la BD viva: solo `SELECT`/`COUNT`/`EXCEPT`; ningún `INSERT`/`UPDATE`/`DELETE`/`DDL`.
   El `run_pg_query`/`run_sql` existente ya permite SQL arbitrario; el comando nuevo **construye
   internamente** queries de solo lectura y **valida** que no contengan palabras de escritura
   (defensa extra; la construcción interna ya lo garantiza).
2. **Nombres validados**: `pg_utils::validate_table_name` y equivalente MariaDB (solo `[a-z0-9_]`)
   antes de interpolar cualquier nombre en SQL. Nunca se interpola contenido de filas.
3. **Secrets**: credenciales solo en el comando docker del VPS; nunca en logs ni en el reporte
   (`infra::secrets::redact_text`). El dump restaurado en el contenedor temporal puede contener
   datos, pero el contenedor se destruye al final y no se exporta su contenido fuera del diff.
4. **Aislamiento**: el contenedor temporal no publica puertos, no está en la red del stack y se
   elimina con `--rm` + `finally` (doble garantía).
5. **Límites**: `--limit-diff` (muestra), timeout global, límite de tamaño de dump a restaurar
   (por defecto 500 MB, configurable), y aviso si el dump es más reciente que la BD viva.

---

## 5. Casos especiales (los que rompieron el método manual)

| Caso | Manejo |
|---|---|
| **pgvector** (`vector`) | Detecta columnas `vector`; compara la tabla sin esa columna; reporta `vector_column_ignored: true` y lo suma a `no_comparables` si es la única vía |
| **Tablas sin PK** | `EXCEPT` funciona sin PK; la muestra se ordena por proyección canónica de columnas |
| **`bytea` / texto enorme** | `EXCEPT` nativo compara bytes; sin parsing de strings |
| **Columnas volátiles** (updated_at, seed timestamps) | `--ignore-columns` para excluirlas de la proyección de comparación (ambos lados) |
| **Spam / contenido no aprobado** (WP) | Opción `--where` global o por tabla (p. ej. `post_status='publish'`) para comparar solo contenido relevante |
| **Auto-increment / serials** que difieren | Los IDs se comparan como datos normales; el reporte muestra si el diff es solo de IDs o de contenido |
| **Tablas de logging/estado efímero** | No se ocultan, pero el resumen permite `--exclude-tables` con prefijo (p. ej. `pg_`, `_sqlx_migrations` opcional) |
| **Dump binario (`pg_dump -Fc`)** | Detectar cabecera y usar `pg_restore` en el contenedor temporal; o si es texto plano, `psql` |
| **Dump gzip** | `gunzip -c` en el VPS antes de pipetear al contenedor temporal |
| **Sitio sin dump todavía** (p. ej. `task`) | Mensaje claro: "no hay dump diario/semanal para <stack_uuid>"; sugerir `backup` primero |

---

## 6. Fases de implementación (verificables)

### Fase A — Esqueleto y descubrimiento de esquema (bloque 1)
- `src/commands/db_compare.rs` + `compare/schema.rs` + registro en CLI/dispatch.
- Soporta solo `--tables`/conteos + listado de esquema de ambos lados (vivo y dump solo conteos).
- **Verificación**: `db-compare --name studio --json` lista esquema real de studio; unit tests de
  `schema.rs` con fixtures SQL.

### Fase B — Modo ligero (`--no-tmp-container`)
- `compare/digest.rs`: conteos + hash canónico con manejo de `vector`/`bytea`.
- **Verificación**: correr contra `agape` y `glory-rest` (se espera idéntico a su dump) y contra
  `studio` (se espera diferencia masiva) → resultados coherentes con el incidente.

### Fase C — Contenedor temporal + diff SQL (núcleo preciso)
- `infra/db_tmp.rs` + `compare/diff.rs`: restaurar dump, `EXCEPT` bidireccional, muestreo, cleanup.
- Soporte PG primero; MariaDB después (mismo patrón).
- **Verificación**: comparar `glory-rest` vs su dump → `tablas_identicas` todas; inyectar una fila
  de prueba en el contenedor temporal (no en vivo) y confirmar que el diff la detecta (test de
  precisión).

### Fase D — Reporte, límites y endurecimiento
- `compare/report.rs` (JSON estable), `--limit-diff`, timeouts, redacción de secrets, validación de
  nombres, exclusión de columnas.
- **Verificación**: ejecutar contra `kamples` (con pgvector) → debe reportar `vector_column_ignored`
  sin fallar; contra `padel` (spam en comentarios) con `--where` → diff limpio.

### Fase E — MCP tool + documentación + roadmap
- Exponer en `src/mcp/tools.rs` si aplica; actualizar README y `roadmap.md` (mover E12 a completado
  cuando se cierre); prueba final de regresión (build + tests del área).

**Definition of Done (DoD):**
- [x] `db-compare` funciona contra PG y MariaDB, con dump VPS y contra otro sitio.
- [x] Descubrimiento automático: funciona sin conocer tablas de antemano (probado en glory-rest con
      tablas custom `bdp_*`, `reservas`, `campanas`, etc. y en guillermo con tablas WP).
- [x] 100% solo lectura en BD viva (revisado en el código del comando).
- [x] Contenedor temporal siempre limpiado (verificado en producción: 0 contenedores y 0 dumps
      residuales tras ejecuciones reales).
- [x] Salida JSON estable documentada y probada con `--json`.
- [x] Tests unitarios del área (11 verdes) + verificación funcional contra sitios reales (agape,
      glory-rest, guillermo, studio) — evidencia en `Agente/completados/tareas-2026-08-28.md`.

---

## 7. Riesgos y mitigaciones

| Riesgo | Mitigación |
|---|---|
| Restaurar un dump corrupto en el contenedor temporal | Detecta error de restauración, limpia el contenedor y reporta `"dump_invalido": true` sin tocar nada más |
| El dump contiene datos sensibles y el reporte los muestra | `--limit-diff` bajo por defecto (20), muestra truncada, redacción de secretos; el contenedor se destruye |
| Contenedor temporal sin limpiar (crash) | `--rm` + `finally` + prefijo identificable `coolify-dbcompare-*`; comando de barrido `db-compare --cleanup-tmp` |
| Dump demasiado grande | Límite configurable (default 500 MB); aviso antes de restaurar |
| pgvector ausente en la imagen temporal | Degrada esas tablas con alerta explícita; nunca aborta todo |
| Coste/tiempo de restaurar dumps grandes | Modo ligero como triage rápido; el modo completo es opt-in por sitio |

---

## 8. Entregables

1. Código: `db_compare.rs`, `compare/`, `db_tmp.rs`, registro CLI/dispatch/MCP.
2. Tests unitarios del área (`schema`, `digest`, `diff`, `report`, `db_tmp`).
3. Documentación: README + sección en `roadmap.md` (E12) + entrada en `Agente/completados/` con
   evidencia de la verificación funcional.
4. Evidencia reproducible: comandos ejecutados y reportes JSON contra sitios reales (agape,
   glory-rest, kamples, studio) comparados con sus dumps VPS.

---

## 9. Pendiente de decisión del operador (RESUELTO 28/08)

- [x] **Firma del comando aprobada**: `db-compare --name <sitio> [--dump <ruta>] [--against <otro>]
      [--tables t1,t2] [--ignore-columns c1,c2] [--limit-diff N] [--json] [--no-tmp-container]
      [--extract-limit N]`.
- [x] **Límite de dump**: se optó por `--extract-limit` (máx filas por tabla) en lugar de un límite
      de tamaño de dump; el modo ligero es el triage rápido sin contenedor.
- [x] **Exposición MCP**: sí — tool `coolify_db_compare` (schema con `site_name` requerido).
- [x] **Prioridad**: ambos motores implementados en paralelo (PG y MariaDB).

---

## 10. Cierre (28/08)

Implementado, compilado sin warnings, 11 tests unitarios verdes y **verificado en producción**
(solo lectura) contra:

| Sitio | Motor | Resultado |
|---|---|---|
| agape | postgres | 13/13 idénticas (modo ligero) |
| glory-rest | postgres | 40/40 idénticas vs dump 28/08 |
| guillermo | mariadb | 11 idénticas + wp_options (cron/transients) |
| studio | postgres | 53 idénticas + 3 con diffs de telemetría/timestamps |

Limpieza verificada: 0 contenedores `coolify-dbcompare-*` y 0 dumps `/tmp/dbcompare_*` tras las
 ejecuciones. Documentación actualizada: `roadmap.md` (E12 implementada), `README.md`
 (§ `db-compare`), `Agente/documentacion/incidente-backups-2026-08-27.md` (§10) y registro en
 `Agente/completados/tareas-2026-08-28.md`.
