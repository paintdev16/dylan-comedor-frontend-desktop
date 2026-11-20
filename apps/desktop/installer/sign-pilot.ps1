param(
    [Parameter(Mandatory)][string]$Bundle,
    [Parameter(Mandatory)][string]$Signer,
    [Parameter(Mandatory)][string]$RawKey
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$keyPath = (Resolve-Path -LiteralPath $RawKey).Path
if ($keyPath.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'La clave debe estar fuera del repositorio'
}
if ((Get-Item -LiteralPath $keyPath).Length -ne 32) { throw 'Veltrix requiere una clave binaria de 32 bytes' }
$Signer = (Resolve-Path -LiteralPath $Signer).Path
$Bundle = (Resolve-Path -LiteralPath $Bundle).Path
$manifestPath = Join-Path $Bundle 'update-windows-x86_64.json'
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
$stable = $manifest.version -match '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if (-not $stable -and $manifest.version -notmatch '^1\.0\.0-beta\.[1-9][0-9]*$') { throw 'Versión inválida' }
$configName = if ($stable) { 'app-stable.json' } else { 'app.json' }
$config = Get-Content -Raw (Join-Path $PSScriptRoot $configName) | ConvertFrom-Json
$name = "Dylan-Setup-$($manifest.version)-x64.exe"
$url = "https://github.com/paintdev16/dylan-comedor-frontend-desktop/releases/download/v$($manifest.version)/$name"
if ($manifest.url -cne $url -or $manifest.app_id -cne $config.app_id -or $manifest.channel -cne $config.channel -or
    $manifest.platform -cne 'windows' -or $manifest.architecture -cne 'x86_64') { throw 'Manifiesto fuera de la política de Dylan Test' }
$setup = Join-Path $Bundle $name
$dylan = Join-Path $Bundle 'inputs/dylan.exe'
if (-not (Test-Path -LiteralPath $dylan)) { $dylan = Join-Path $Bundle 'dylan.exe' }
if ((Get-FileHash -LiteralPath $dylan -Algorithm SHA256).Hash -ine $manifest.installed_sha256) {
    throw 'El hash firmado del ejecutable no corresponde al archivo compilado'
}
$workingSignature = Join-Path $Bundle ([guid]::NewGuid().ToString() + '.sig')
try {
    $output = & $Signer $keyPath $manifestPath $setup $workingSignature 2>&1 | Out-String
    if ($LASTEXITCODE) { throw "Falló veltrix-sign: $output" }
    if ($output -notmatch '(?m)^Public key: ([a-fA-F0-9]{64})\s*$' -or $Matches[1] -ine $config.public_key) {
        throw 'La clave privada no corresponde a la clave pública de Dylan Test'
    }
    if ((Get-Item -LiteralPath $workingSignature).Length -ne 64) { throw 'Firma inválida' }
    Move-Item -LiteralPath $workingSignature -Destination "$manifestPath.sig"
} finally {
    if (Test-Path -LiteralPath $workingSignature) { Remove-Item -LiteralPath $workingSignature }
}
