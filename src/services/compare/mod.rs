/*
 * compare — motor de comparación de bases de datos (E12).
 * Descubre esquemas, extrae filas canónicas y produce diffs precisos
 * sin parsear dumps SQL como texto.
 */

pub mod diff;
pub mod digest;
pub mod report;
pub mod schema;
