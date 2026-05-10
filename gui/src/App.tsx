/*
 * App — consola operativa de Coolify Manager.
 */

import { BarraLateral } from "./componentes/BarraLateral";
import { VistaSitios } from "./componentes/VistaSitios";
import "./estilos/layout.css";
import "./estilos/componentes.css";

export function App() {
    return (
        <div className="contenedorLayout">
            <BarraLateral />
            <main className="contenidoPrincipal">
                <VistaSitios />
            </main>
        </div>
    );
}
