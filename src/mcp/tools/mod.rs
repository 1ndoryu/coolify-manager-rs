/*
 * MCP Tools — routers y definiciones de wire (split 119A-3 de tools.rs).
 * `definiciones`: tablas del catálogo (`list_tools`); `despacho`: routers
 * `call_tool`/`despachar_*` por dominio. Se re-exporta plano para preservar
 * las rutas (`tools::list_tools`, `tools::call_tool`).
 */

pub mod definiciones;
pub mod despacho;

pub use definiciones::list_tools;
pub use despacho::call_tool;
