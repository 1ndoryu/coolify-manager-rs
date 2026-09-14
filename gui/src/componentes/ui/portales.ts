/* Raíz de montaje para portales React (modales, menús contextuales).
 * [P2-039A-1] Punto único de montaje: hoy es `document.body`; si el shell
 * necesita un contenedor propio, solo cambia aquí. */
export function obtenerRaizPortales(): HTMLElement {
    return document.body;
}
