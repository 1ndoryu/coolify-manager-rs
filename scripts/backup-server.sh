#!/bin/bash
# backup-server.sh — Backup automático auto-descubridor de bases de datos
# Instalar en: /usr/local/bin/backup-server.sh (vía coolify-manager install-backups)
# Crontab root: 0 3 * * * /usr/local/bin/backup-server.sh
#
# ZERO HARDCODING: detecta automáticamente todos los containers PostgreSQL y MariaDB.
# No importa si agregas, eliminas o renombras sitios — el script los encuentra solos.
#
# Configuración por sitio (opcional):
#   /etc/backup-sites.conf — overrides por stack UUID.
#   Si no existe, usa defaults (2 daily, 2 weekly, 500MB threshold).
#   Formato: STACK_UUID|daily_keep|weekly_keep|max_daily_mb
#
# Política default:
#   - Diario (lun-sáb): 2 últimos dumps por sitio (≤500MB)
#   - Semanal (dom): 2 últimos dumps por sitio (≤500MB)
#   - Sitios con dumps >500MB: 1 semanal máximo (sin daily)
#   - Organización: /data/backups/{stack_uuid}/{daily|weekly}/
#
# Ejecución manual:
#   backup-server.sh                    # Todos los containers encontrados
#   backup-server.sh --site studio      # Match por nombre parcial
#   backup-server.sh --tier daily       # Forzar tier
#   backup-server.sh --dry-run          # Solo listar containers, sin backup

set -euo pipefail

BACKUP_ROOT="/data/backups"
LOG_FILE="${BACKUP_ROOT}/backup.log"
CONFIG_FILE="/etc/backup-sites.conf"

# Defaults (overrideables por sitio en $CONFIG_FILE)
DEFAULT_DAILY_KEEP=2
DEFAULT_WEEKLY_KEEP=2
DEFAULT_MAX_DAILY_MB=500
DEFAULT_HEAVY_WEEKLY_KEEP=1

# --- Parse args ---
TARGET_SITE=""
TARGET_TIER=""
DRY_RUN=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --site) TARGET_SITE="$2"; shift 2 ;;
    --tier) TARGET_TIER="$2"; shift 2 ;;
    --dry-run) DRY_RUN=true; shift ;;
    *) echo "Unknown arg: $1"; exit 1 ;;
  esac
done

mkdir -p "${BACKUP_ROOT}"

log() {
  local msg
  msg="$(date -u '+%Y-%m-%d %H:%M:%S') | $1"
  echo "$msg" | tee -a "${LOG_FILE}"
}

# Leer config por sitio desde /etc/backup-sites.conf
# Formato: STACK_UUID|daily_keep|weekly_keep|max_daily_mb
read_site_config() {
  local stack_uuid="$1"
  local daily_keep=$DEFAULT_DAILY_KEEP
  local weekly_keep=$DEFAULT_WEEKLY_KEEP
  local max_daily_mb=$DEFAULT_MAX_DAILY_MB

  if [[ -f "$CONFIG_FILE" ]]; then
    while IFS='|' read -r uuid dk wkm mdm || [[ -n "$uuid" ]]; do
      [[ "$uuid" =~ ^[[:space:]]*# ]] && continue
      [[ -z "${uuid// /}" ]] && continue
      uuid="${uuid// /}"
      if [[ "$uuid" == "$stack_uuid" ]]; then
        daily_keep="${dk:-$DEFAULT_DAILY_KEEP}"
        weekly_keep="${wkm:-$DEFAULT_WEEKLY_KEEP}"
        max_daily_mb="${mdm:-$DEFAULT_MAX_DAILY_MB}"
        break
      fi
    done < "$CONFIG_FILE"
  fi

  echo "$daily_keep $weekly_keep $max_daily_mb"
}

rotate() {
  local dir="$1"
  local keep="$2"
  local count
  count=$(find "$dir" -name '*.sql.gz' -type f 2>/dev/null | wc -l)
  if (( count > keep )); then
    find "$dir" -name '*.sql.gz' -type f -printf '%T@ %p\n' \
      | sort -n \
      | head -n "$((count - keep))" \
      | awk '{print $2}' \
      | xargs -r rm -f
    log "ROTATE | ${dir} | kept=${keep} removed=$((count - keep))"
  fi
}

# Extraer variable de entorno de un container
get_env() {
  docker exec "$1" printenv "$2" 2>/dev/null || echo ""
}

# Determinar tier según día y tamaño
resolve_tier() {
  local size_mb="$1"
  local max_daily_mb="$2"

  if [[ -n "$TARGET_TIER" ]]; then
    echo "$TARGET_TIER"
    return
  fi

  if (( size_mb > max_daily_mb )); then
    echo "throttle"
    return
  fi

  local dow
  dow=$(date -u '+%u')
  if [[ "$dow" == "7" ]]; then
    echo "weekly"
  else
    echo "daily"
  fi
}

# Backup de un container PostgreSQL
backup_postgres() {
  local container="$1"
  local stack_uuid="$2"
  local timestamp
  timestamp="$(date -u '+%Y-%m-%d_%H%M')"

  # Credenciales del propio container (zero hardcoding)
  local db_user db_name
  db_user=$(get_env "$container" "POSTGRES_USER")
  db_name=$(get_env "$container" "POSTGRES_DB")

  if [[ -z "$db_user" || -z "$db_name" ]]; then
    log "SKIP | ${container} | POSTGRES_USER/POSTGRES_DB not set"
    return 1
  fi

  # Config por sitio
  local config
  config=$(read_site_config "$stack_uuid")
  local daily_keep weekly_keep max_daily_mb
  read -r daily_keep weekly_keep max_daily_mb <<< "$config"

  # Container running?
  local state
  state=$(docker inspect -f '{{.State.Status}}' "$container" 2>/dev/null || echo "missing")
  if [[ "$state" != "running" ]]; then
    log "SKIP | ${container} | state=${state}"
    return 1
  fi

  local tmp_file
  tmp_file=$(mktemp /tmp/backup-XXXXXX.sql.gz)

  if ! docker exec "$container" pg_dump -U "$db_user" -d "$db_name" --no-owner --no-privileges \
    | gzip > "$tmp_file" 2>/dev/null; then
    log "FAILED | ${container} | pg_dump failed"
    rm -f "$tmp_file"
    return 1
  fi

  local size_bytes
  size_bytes=$(stat -c%s "$tmp_file" 2>/dev/null || echo 0)
  if (( size_bytes < 100 )); then
    log "EMPTY | ${container} | ${size_bytes} bytes"
    rm -f "$tmp_file"
    return 1
  fi

  local size_mb=$((size_bytes / 1048576))
  local tier_result
  tier_result=$(resolve_tier "$size_mb" "$max_daily_mb")

  local tier keep
  if [[ "$tier_result" == "throttle" ]]; then
    tier="weekly"
    keep=$DEFAULT_HEAVY_WEEKLY_KEEP
    log "THROTTLE | ${container} | ${size_mb}MB > ${max_daily_mb}MB → weekly only"
  else
    tier="$tier_result"
    if [[ "$tier" == "daily" ]]; then
      keep=$daily_keep
    else
      keep=$weekly_keep
    fi
  fi

  local dest_dir="${BACKUP_ROOT}/${stack_uuid}/${tier}"
  mkdir -p "$dest_dir"
  local dest_file="${dest_dir}/${timestamp}.sql.gz"
  mv "$tmp_file" "$dest_file"
  rotate "$dest_dir" "$keep"

  log "OK | ${container} | ${tier} | ${size_mb}MB | ${dest_file##*/}"
}

# Backup de un container MariaDB
backup_mariadb() {
  local container="$1"
  local stack_uuid="$2"
  local timestamp
  timestamp="$(date -u '+%Y-%m-%d_%H%M')"

  # Credenciales del propio container (zero hardcoding)
  local db_user db_pass db_name
  db_user=$(get_env "$container" "MARIADB_USER")
  db_pass=$(get_env "$container" "MARIADB_PASSWORD")
  db_name=$(get_env "$container" "MARIADB_DATABASE")

  # Fallback: algunos containers usan MYSQL_*
  [[ -z "$db_user" ]] && db_user=$(get_env "$container" "MYSQL_USER")
  [[ -z "$db_pass" ]] && db_pass=$(get_env "$container" "MYSQL_PASSWORD")
  [[ -z "$db_name" ]] && db_name=$(get_env "$container" "MYSQL_DATABASE")

  if [[ -z "$db_user" || -z "$db_name" ]]; then
    log "SKIP | ${container} | no MARIADB_USER/MYSQL_USER or DATABASE"
    return 1
  fi

  local config
  config=$(read_site_config "$stack_uuid")
  local daily_keep weekly_keep max_daily_mb
  read -r daily_keep weekly_keep max_daily_mb <<< "$config"

  local state
  state=$(docker inspect -f '{{.State.Status}}' "$container" 2>/dev/null || echo "missing")
  if [[ "$state" != "running" ]]; then
    log "SKIP | ${container} | state=${state}"
    return 1
  fi

  local tmp_file
  tmp_file=$(mktemp /tmp/backup-XXXXXX.sql.gz)

  local dump_cmd
  if [[ -n "$db_pass" ]]; then
    dump_cmd="mariadb-dump -u '${db_user}' -p'${db_pass}' --single-transaction --routines --triggers '${db_name}'"
  else
    dump_cmd="mariadb-dump -u '${db_user}' --single-transaction --routines --triggers '${db_name}'"
  fi

  if ! docker exec "$container" sh -c "$dump_cmd" | gzip > "$tmp_file" 2>/dev/null; then
    log "FAILED | ${container} | mariadb-dump failed"
    rm -f "$tmp_file"
    return 1
  fi

  local size_bytes
  size_bytes=$(stat -c%s "$tmp_file" 2>/dev/null || echo 0)
  if (( size_bytes < 100 )); then
    log "EMPTY | ${container} | ${size_bytes} bytes"
    rm -f "$tmp_file"
    return 1
  fi

  local size_mb=$((size_bytes / 1048576))
  local tier_result
  tier_result=$(resolve_tier "$size_mb" "$max_daily_mb")

  local tier keep
  if [[ "$tier_result" == "throttle" ]]; then
    tier="weekly"
    keep=$DEFAULT_HEAVY_WEEKLY_KEEP
    log "THROTTLE | ${container} | ${size_mb}MB > ${max_daily_mb}MB → weekly only"
  else
    tier="$tier_result"
    if [[ "$tier" == "daily" ]]; then
      keep=$daily_keep
    else
      keep=$weekly_keep
    fi
  fi

  local dest_dir="${BACKUP_ROOT}/${stack_uuid}/${tier}"
  mkdir -p "$dest_dir"
  local dest_file="${dest_dir}/${timestamp}.sql.gz"
  mv "$tmp_file" "$dest_file"
  rotate "$dest_dir" "$keep"

  log "OK | ${container} | ${tier} | ${size_mb}MB | ${dest_file##*/}"
}

# --- Auto-descubrimiento ---

discover_postgres_containers() {
  docker ps --format '{{.Names}}' --filter "status=running" 2>/dev/null \
    | grep '^postgres-' \
    | sort || true
}

discover_mariadb_containers() {
  docker ps --format '{{.Names}}' --filter "status=running" 2>/dev/null \
    | grep -i 'mariadb' \
    | sort || true
}

extract_stack_uuid() {
  # postgres-{uuid} → {uuid}
  echo "${1#postgres-}"
}

# --- Main ---
log "=== BACKUP RUN START ==="

# Disk space check (need ≥1GB free)
free_kb=$(df -k "${BACKUP_ROOT}" | awk 'NR==2{print $4}')
if (( free_kb < 1048576 )); then
  log "DISK_LOW | ${BACKUP_ROOT} has $((free_kb/1024))MB free — aborting"
  exit 1
fi

errors=0
total=0

# --- PostgreSQL ---
while IFS= read -r container; do
  [[ -z "$container" ]] && continue
  local_uuid=$(extract_stack_uuid "$container")

  if [[ -n "$TARGET_SITE" ]]; then
    if [[ "$container" != *"$TARGET_SITE"* && "$local_uuid" != *"$TARGET_SITE"* ]]; then
      continue
    fi
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] PostgreSQL: $container (uuid=$local_uuid)"
  else
    ((total++))
    backup_postgres "$container" "$local_uuid" || ((errors++))
  fi
done < <(discover_postgres_containers)

# --- MariaDB ---
while IFS= read -r container; do
  [[ -z "$container" ]] && continue
  local_uuid=$(docker inspect -f '{{.Name}}' "$container" 2>/dev/null \
    | sed 's|^/||' | grep -oP '[a-z0-9]{25}' | head -1 || echo "$container")

  if [[ -n "$TARGET_SITE" ]]; then
    if [[ "$container" != *"$TARGET_SITE"* && "$local_uuid" != *"$TARGET_SITE"* ]]; then
      continue
    fi
  fi

  if [[ "$DRY_RUN" == "true" ]]; then
    echo "[dry-run] MariaDB: $container (uuid=$local_uuid)"
  else
    ((total++))
    backup_mariadb "$container" "$local_uuid" || ((errors++))
  fi
done < <(discover_mariadb_containers)

if [[ "$DRY_RUN" == "true" ]]; then
  echo "[dry-run] Re-run without --dry-run to execute"
fi

log "=== BACKUP RUN END | total=${total} errors=${errors} ==="
exit $errors
