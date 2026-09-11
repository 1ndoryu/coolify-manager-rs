# Etapa cargo del gate (119A-1): fija CARGO_TARGET_DIR fuera del arbol (C:\tmp),
# ejecuta el cargo del PATH (el gate exime a la etapa del guard via lease) y,
# si el subcomando termina en 0, emite el reporte findings vacio que exige el gate.
# En fallo propaga el codigo de cargo; el gate conserva stdout/stderr en el log.
# Uso: cargo-stage.ps1 <reportPath> <args de cargo...>
# (sin bloque param: con pwsh -File los argumentos llegan en $args).
$Report = $args[0]
$cargoArgs = @($args | Select-Object -Skip 1)
$target = $env:GLORY_CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($target)) { $target = 'C:\tmp\glory-target\coolify-manager' }
$env:CARGO_TARGET_DIR = $target
& cargo @cargoArgs
$code = $LASTEXITCODE
if ($code -eq 0) {
    Set-Content -LiteralPath $Report -Value '{"schemaVersion":1,"entries":[]}' -Encoding utf8 -NoNewline
}
exit $code
