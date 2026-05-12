/*
 * [125A-3] VistaLogin — pantalla de acceso a la consola operativa.
 * Usada como guard en App.tsx si !autenticado.
 * CSS exclusivo en login.css usando variables del sistema de diseño.
 */

import { useState } from "react";
import "../estilos/login.css";

interface VistaLoginProps {
    onLogin: (email: string, password: string) => Promise<boolean>;
    error: string | null;
}

export function VistaLogin({ onLogin, error }: VistaLoginProps) {
    const [email, setEmail] = useState("");
    const [password, setPassword] = useState("");
    const [cargando, setCargando] = useState(false);

    async function handleSubmit(e: React.FormEvent) {
        e.preventDefault();
        setCargando(true);
        await onLogin(email, password);
        setCargando(false);
    }

    return (
        <div className="contenedorLogin">
            <div className="panelLogin">
                <h1 className="tituloLogin">Coolify Manager</h1>
                <form onSubmit={handleSubmit} className="formularioLogin">
                    <div className="campoLogin">
                        <label htmlFor="loginEmail">Email</label>
                        <input
                            id="loginEmail"
                            type="email"
                            value={email}
                            onChange={e => setEmail(e.target.value)}
                            required
                            autoComplete="email"
                            disabled={cargando}
                        />
                    </div>
                    <div className="campoLogin">
                        <label htmlFor="loginPassword">Contraseña</label>
                        <input
                            id="loginPassword"
                            type="password"
                            value={password}
                            onChange={e => setPassword(e.target.value)}
                            required
                            autoComplete="current-password"
                            disabled={cargando}
                        />
                    </div>
                    {error && <p className="errorLogin">{error}</p>}
                    <button type="submit" className="botonLogin" disabled={cargando || !email || !password}>
                        {cargando ? "Verificando…" : "Entrar"}
                    </button>
                </form>
            </div>
        </div>
    );
}
