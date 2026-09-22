/*
 * compare_manager — orquesta la comparación de bases de datos (E12).
 *
 * Flujo:
 *   1. Resolver motor y credenciales del sitio vivo (PG o MariaDB).
 *   2. Descubrir esquema de la BD viva (automático, sin hardcodear tablas).
 *   3. Según el objetivo:
 *      - dump VPS/local → modo ligero (conteos+hash) o modo completo
 *        (restaura en contenedor temporal efímero y compara con SQL real).
 *      - otro sitio en vivo → compara las dos BDs directamente.
 *   4. Producir reporte JSON estable.
 *
 * Garantías: SOLO LECTURA sobre la BD viva; contenedor temporal SIEMPRE
 * limpiado; nombres de tablas validados; secrets nunca en el reporte.
 *
 * Split 119A-5: tipos.rs + origen.rs + dumps.rs + ligero.rs + completo.rs;
 * re-exports sin cambios para no tocar callers externos.
 */

mod completo;
mod dumps;
mod ligero;
mod origen;
mod tipos;

pub use completo::execute;
pub use ligero::_build_light_report;
pub use tipos::CompareOptions;
