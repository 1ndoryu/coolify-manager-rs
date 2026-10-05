#!/bin/bash
# ssh-guard.sh - CM_GUARD_v1 (299A-1).
# Wrapper server-side: veta comandos destructivos antes de ejecutarlos.
# Uso: ssh-guard.sh '<comando>'.
# NO es ForceCommand de sshd: un bug aqui no bloquea el acceso SSH.
# Log: /var/log/coolify-guard.log (si no hay permiso, /opt/coolify-guard/guard.log).
# Override: GUARD_ALLOW_REBOOT=1 permite reboot/poweroff/halt/shutdown
# (lo usa el mantenimiento del manager; ver maintenance_render.rs).
# Solo ASCII puro (leccion 268A-5: el validador ASCII de Coolify no aplica aqui,
# pero se conserva por higiene en transfers base64).

GUARD_LOG="/var/log/coolify-guard.log"
if [ ! -w "$(dirname "$GUARD_LOG")" ] 2>/dev/null; then
    GUARD_LOG="/opt/coolify-guard/guard.log"
fi

guard_log() {
    echo "$(date -Is) $1 cmd=$2" >> "$GUARD_LOG" 2>/dev/null || true
}

CMD="$1"
if [ -z "$CMD" ]; then
    echo "ssh-guard: sin comando" >&2
    exit 125
fi

# Denylist conservadora (grep -E POSIX, sin \s):
#  rm -r/-f/--recursive contra /  | mkfs | dd hacia /dev/*
#  fork-bomb  | chmod -R 777 /  | reboot/poweroff/halt/shutdown/init 0|6
if echo "$CMD" | grep -Eq 'rm[[:space:]]+(-[a-zA-Z]*[rf][a-zA-Z]*|--recursive)[[:space:]]+/([[:space:]]|;|$)'; then
    guard_log "DENY rm-root" "$CMD"
    echo "ssh-guard: comando vetado (rm contra /)" >&2
    exit 126
fi
if echo "$CMD" | grep -Eq 'mkfs(\.|$)|dd[[:space:]]+.*of=/dev/|:\(\)\{'; then
    guard_log "DENY disco" "$CMD"
    echo "ssh-guard: comando vetado (disco/bomba)" >&2
    exit 126
fi
if echo "$CMD" | grep -Eq 'chmod[[:space:]]+-R[[:space:]]+777[[:space:]]+/([[:space:]]|;|$)'; then
    guard_log "DENY chmod" "$CMD"
    echo "ssh-guard: comando vetado (chmod 777 /)" >&2
    exit 126
fi
if echo "$CMD" | grep -Eq '(^|[;&|][[:space:]]*)(reboot|poweroff|halt|shutdown|init[[:space:]]+[06])([[:space:]]|;|$)'; then
    if [ "$GUARD_ALLOW_REBOOT" = "1" ]; then
        guard_log "ALLOW-reboot" "$CMD"
        exec bash -c "$CMD"
    fi
    guard_log "DENY reboot" "$CMD"
    echo "ssh-guard: comando vetado (reinicio; usar GUARD_ALLOW_REBOOT=1)" >&2
    exit 126
fi

guard_log "ALLOW" "$CMD"
exec bash -c "$CMD"
