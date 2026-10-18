param(
    [string[]]$TargetDirs = @('C:\tmp\glory-target'),
    [string[]]$ExcludeDirs = @(),
    [int]$MaxTotalMB = 4096,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Normalize-Path {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $null
    }

    try {
        return [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
    } catch {
        return $Path.TrimEnd('\')
    }
}

function Test-IsExcludedPath {
    param(
        [string]$Path,
        [string[]]$ExcludedPrefixes
    )

    $normalizedPath = Normalize-Path -Path $Path
    if (-not $normalizedPath) {
        return $false
    }

    foreach ($excluded in $ExcludedPrefixes) {
        if (-not $excluded) {
            continue
        }

        if ($normalizedPath.Equals($excluded, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }

        if ($normalizedPath.StartsWith("$excluded\", [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }

    return $false
}

function Get-DirectorySizeMB {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        return 0
    }

    $totalBytes = 0
    Get-ChildItem -LiteralPath $Path -Recurse -Force -File -ErrorAction SilentlyContinue | ForEach-Object {
        $totalBytes += $_.Length
    }
    return [math]::Round($totalBytes / 1MB, 2)
}

function Remove-MatchingDirectories {
    param(
        [string]$Path,
        [string[]]$Names,
        [string[]]$ExcludedPrefixes
    )

    foreach ($name in $Names) {
        Get-ChildItem -LiteralPath $Path -Recurse -Force -Directory -ErrorAction SilentlyContinue |
            Where-Object {
                $_.Name -eq $name -and -not (Test-IsExcludedPath -Path $_.FullName -ExcludedPrefixes $ExcludedPrefixes)
            } |
            Sort-Object FullName -Descending |
            ForEach-Object {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction SilentlyContinue
            }
    }
}

function Test-RustBuildActive {
    return [bool](Get-Process -Name 'cargo', 'rustc' -ErrorAction SilentlyContinue)
}

$normalizedExcludeDirs = @($ExcludeDirs | ForEach-Object { Normalize-Path -Path $_ } | Where-Object { $_ })

foreach ($targetDir in $TargetDirs) {
    if (-not (Test-Path $targetDir)) {
        continue
    }

    $initialSize = Get-DirectorySizeMB -Path $targetDir
    if ($initialSize -le $MaxTotalMB) {
        Write-Host "[cargo-clean] $targetDir OK ($initialSize MB / $MaxTotalMB MB)"
        continue
    }

    if ((Test-RustBuildActive) -and (-not $Force)) {
        Write-Host "[cargo-clean] build Rust activo; se pospone limpieza de $targetDir ($initialSize MB)"
        continue
    }

    Write-Host "[cargo-clean] limpiando $targetDir ($initialSize MB / $MaxTotalMB MB)"
    Remove-MatchingDirectories -Path $targetDir -Names @('incremental') -ExcludedPrefixes $normalizedExcludeDirs

    $afterIncremental = Get-DirectorySizeMB -Path $targetDir
    if ($afterIncremental -le $MaxTotalMB) {
        Write-Host "[cargo-clean] $targetDir reducido a $afterIncremental MB"
        continue
    }

    Remove-MatchingDirectories -Path $targetDir -Names @('.fingerprint', 'build') -ExcludedPrefixes $normalizedExcludeDirs

    $afterMetadata = Get-DirectorySizeMB -Path $targetDir
    if ($afterMetadata -le $MaxTotalMB) {
        Write-Host "[cargo-clean] $targetDir reducido a $afterMetadata MB"
        continue
    }

    Remove-MatchingDirectories -Path $targetDir -Names @('deps') -ExcludedPrefixes $normalizedExcludeDirs
    $finalSize = Get-DirectorySizeMB -Path $targetDir
    Write-Host "[cargo-clean] $targetDir reducido a $finalSize MB"
}