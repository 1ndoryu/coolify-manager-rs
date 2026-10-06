/* Volumen persistente pg_data en el servicio postgres [06AA-4 split compose_sync]. */

/// [21C-7] Inyecta `- pg_data:/var/lib/postgresql/data` en el servicio postgres si falta.
/// Stacks legacy pueden no tener el volumen montado → E18 bloquea el deploy.
/// Busca el bloque `postgres:` dentro de `services:`, luego su sub-bloque `volumes:`.
/// Si no existe `volumes:` en postgres, lo crea. Si existe pero falta el mount, lo agrega.
/* [119A-2] Pasada 1 de inject_postgres_data_volume: detectar si el servicio
 * postgres ya tiene bloque `volumes:`. */
fn detectar_bloque_volumes_postgres(lineas: &[&str]) -> bool {
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut tiene_bloque = false;
    let mut indent_volumes: usize = 0;

    for linea in lineas {
        let recortada = linea.trim();
        if recortada.is_empty() || recortada.starts_with('#') {
            continue;
        }
        let indent = linea.len() - linea.trim_start().len();
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = indent as isize;
            continue;
        }
        if en_servicios
            && indent_servicios >= 0
            && indent == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = indent;
            if svc != "postgres" {
                tiene_bloque = false;
            }
        }
        if en_servicios && indent <= indent_servicios as usize && recortada != "services:" {
            en_servicios = false;
            en_postgres = false;
        }
        if en_postgres && indent == indent_postgres + 2 && recortada == "volumes:" {
            tiene_bloque = true;
            indent_volumes = indent;
        }
        /* Fin del bloque volumes: si encontramos algo al mismo nivel o superior */
        if tiene_bloque && indent <= indent_volumes && recortada != "volumes:" {
            tiene_bloque = false;
        }
    }
    tiene_bloque
}

/* [119A-2] Pasada 2: última línea del bloque volumes de postgres (índice
 * para insertar después). Solo se llama si ya existe el bloque. */
fn ultima_linea_volumes_postgres(lineas: &[&str]) -> isize {
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut en_volumes = false;
    let mut indent_vol: usize = 0;
    let mut ultima: isize = -1;

    /* Encontrar la última línea del bloque volumes de postgres */
    for (i, linea) in lineas.iter().enumerate() {
        let recortada = linea.trim();
        if recortada.is_empty() || recortada.starts_with('#') {
            if en_volumes {
                ultima = i as isize;
            }
            continue;
        }
        let indent = linea.len() - linea.trim_start().len();
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = indent as isize;
            continue;
        }
        if en_servicios
            && indent_servicios >= 0
            && indent == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = indent;
            en_volumes = false;
        }
        if en_servicios && indent <= indent_servicios as usize && recortada != "services:" {
            en_servicios = false;
            en_postgres = false;
            en_volumes = false;
        }
        if en_postgres && indent == indent_postgres + 2 && recortada == "volumes:" {
            en_volumes = true;
            indent_vol = indent;
            ultima = i as isize;
        }
        if en_volumes && indent <= indent_vol && recortada != "volumes:" {
            en_volumes = false;
        }
        if en_volumes {
            ultima = i as isize;
        }
    }
    ultima
}

/* [119A-2] Pasada 3: reconstruir el compose inyectando el mount pg_data.
 * Caso A (tiene bloque): inserta después de la última línea del bloque.
 * Caso B (sin bloque): crea el bloque antes de la siguiente clave de postgres. */
fn reconstruir_con_volumen_postgres(
    lineas: &[&str],
    tiene_bloque: bool,
    ultima_volumen: isize,
    termina_con_salto: bool,
) -> String {
    let mut resultado: Vec<String> = Vec::with_capacity(lineas.len() + 2);
    let mut inyectado = false;
    let mut en_servicios = false;
    let mut indent_servicios: isize = -1;
    let mut en_postgres = false;
    let mut indent_postgres: usize = 0;
    let mut en_volumes = false;
    let mut indent_vol: usize = 0;
    let mut actual: isize = -1;

    for linea in lineas {
        actual += 1;
        let recortada = linea.trim();

        /* Caso A: tiene bloque volumes — insertar después de la última línea */
        if tiene_bloque && !inyectado && actual == ultima_volumen + 1 {
            let vol_indent = " ".repeat(indent_postgres + 4);
            resultado.push(format!("{vol_indent}- pg_data:/var/lib/postgresql/data"));
            inyectado = true;
        }

        /* Caso B: no tiene bloque volumes — insertar el bloque completo antes de
         * la siguiente clave al nivel de postgres (environment, depends_on, etc.) */
        if !tiene_bloque && !inyectado && en_postgres {
            let indent = linea.len() - linea.trim_start().len();
            if indent == indent_postgres + 2
                && (recortada.starts_with("environment:")
                    || recortada.starts_with("depends_on:")
                    || recortada.starts_with("healthcheck:")
                    || recortada.starts_with("restart:")
                    || recortada.starts_with("labels:")
                    || recortada.starts_with("image:"))
            {
                let block_indent = " ".repeat(indent_postgres + 2);
                let item_indent = " ".repeat(indent_postgres + 4);
                resultado.push(format!("{block_indent}volumes:"));
                resultado.push(format!("{item_indent}- pg_data:/var/lib/postgresql/data"));
                inyectado = true;
            }
        }

        resultado.push(linea.to_string());

        /* Tracking de contexto (después de push para no duplicar) */
        if recortada == "services:" {
            en_servicios = true;
            indent_servicios = (linea.len() - linea.trim_start().len()) as isize;
        }
        if en_servicios
            && indent_servicios >= 0
            && (linea.len() - linea.trim_start().len()) == (indent_servicios as usize + 2)
            && recortada.ends_with(':')
            && !recortada.contains(' ')
            && !recortada.starts_with('-')
        {
            let svc = recortada.trim_end_matches(':');
            en_postgres = svc == "postgres";
            indent_postgres = linea.len() - linea.trim_start().len();
            en_volumes = false;
        }
        if en_servicios
            && (linea.len() - linea.trim_start().len()) <= indent_servicios as usize
            && recortada != "services:"
        {
            en_servicios = false;
            en_postgres = false;
        }
        if en_postgres
            && (linea.len() - linea.trim_start().len()) == indent_postgres + 2
            && recortada == "volumes:"
        {
            en_volumes = true;
            indent_vol = linea.len() - linea.trim_start().len();
        }
        if en_volumes
            && (linea.len() - linea.trim_start().len()) <= indent_vol
            && recortada != "volumes:"
        {
            en_volumes = false;
        }
    }

    let mut salida = resultado.join("\n");
    if termina_con_salto {
        salida.push('\n');
    }
    if !inyectado {
        eprintln!(
            "[WARN] inject_postgres_data_volume: no se encontró el servicio 'postgres' en el compose. Volumen no inyectado."
        );
    }
    salida
}

pub(super) fn inject_postgres_data_volume(compose: &str) -> String {
    if compose.contains(":/var/lib/postgresql/data") {
        return compose.to_string();
    }
    let ends_with_newline = compose.ends_with('\n');
    let lines: Vec<&str> = compose.lines().collect();
    /* Si ya tiene bloque volumes, insertar despues de la ultima linea del bloque.
     * Si no tiene, necesitamos crear el bloque. */
    let tiene_bloque = detectar_bloque_volumes_postgres(&lines);
    let ultima = if tiene_bloque {
        ultima_linea_volumes_postgres(&lines)
    } else {
        -1
    };
    reconstruir_con_volumen_postgres(&lines, tiene_bloque, ultima, ends_with_newline)
}
