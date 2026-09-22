/* Wrapper redis-cli del auditor (119A-5 split control_plane_audit_manager). */

use super::formato::sh_quote;

pub(super) fn build_redis_cli_script(body: &str) -> String {
    format!(
        "bash -lc {}",
        sh_quote(&format!(
            "set -o pipefail\npassword=$(docker inspect coolify-redis --format '{{{{range .Config.Env}}}}{{{{println .}}}}{{{{end}}}}' 2>/dev/null | awk -F= '$1==\"REDIS_PASSWORD\" {{print substr($0, index($0, \"=\") + 1); exit}}')\nredis_cli() {{\n    if [ -n \"$password\" ]; then\n        docker exec coolify-redis redis-cli --no-auth-warning -a \"$password\" \"$@\"\n    else\n        docker exec coolify-redis redis-cli \"$@\"\n    fi\n}}\n{}",
            body
        ))
    )
}
