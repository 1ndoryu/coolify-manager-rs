import type { ButtonHTMLAttributes, ReactNode } from "react";

type ButtonVariant = "primario" | "secundario" | "icono";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ButtonVariant;
    children: ReactNode;
}

export function Button({ variant = "secundario", className = "", children, ...props }: ButtonProps) {
    const variantClass = variant === "primario" ? "botonPrimario" : variant === "icono" ? "botonIcono" : "botonSecundario";

    return (
        <button className={`boton ${variantClass} ${className}`.trim()} {...props}>
            {children}
        </button>
    );
}

interface IconButtonProps extends Omit<ButtonProps, "variant" | "children"> {
    icon: ReactNode;
}

export function IconButton({ icon, className = "", ...props }: IconButtonProps) {
    /* [105A-12] Base local de transicion hasta mover Button/IconButton a glory-rs compartido. */
    return (
        <Button variant="icono" className={className} {...props}>
            {icon}
        </Button>
    );
}