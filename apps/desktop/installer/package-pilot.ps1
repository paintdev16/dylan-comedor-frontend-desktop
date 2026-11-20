param(
    [Parameter(Mandatory)][string]$MakeNsis,
    [Parameter(Mandatory)][string]$Dylan,
    [Parameter(Mandatory)][string]$Veltrix,
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
$Dylan = (Resolve-Path $Dylan).Path
$Veltrix = (Resolve-Path $Veltrix).Path
$MakeNsis = (Resolve-Path $MakeNsis).Path
$veltrixRelease = Get-Content -Raw (Join-Path $PSScriptRoot 'veltrix-release.json') | ConvertFrom-Json
if ($veltrixRelease.tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$' -or $veltrixRelease.sha256 -notmatch '^[0-9a-f]{64}$') {
    throw 'La versión o el SHA-256 fijado de Veltrix no es válido'
}
if ((Get-FileHash -LiteralPath $Veltrix -Algorithm SHA256).Hash -ine $veltrixRelease.sha256) {
    throw "El ejecutable no coincide con el SHA-256 oficial de Veltrix $($veltrixRelease.tag)"
}
$cargoManifest = Join-Path $PSScriptRoot '../../../Cargo.toml'
$metadata = cargo metadata --manifest-path $cargoManifest --locked --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE) { throw 'No se pudo consultar Cargo' }
$version = ($metadata.packages | Where-Object name -ceq 'dylan').version
$binaryVersion = (& $Dylan --version | Out-String).Trim()
$stable = $version -match '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
if ($LASTEXITCODE -or $version -cne $binaryVersion -or (-not $stable -and $version -notmatch '^1\.0\.0-beta\.[1-9][0-9]*$')) {
    throw "Versión inválida: $version"
}
if ($env:GITHUB_REF_TYPE -eq 'tag' -and $env:GITHUB_REF_NAME -cne "v$version") {
    throw "La etiqueta debe ser v$version"
}
$outputPath = (New-Item -ItemType Directory -Force $Output).FullName
$inputs = (New-Item -ItemType Directory -Force (Join-Path $outputPath 'inputs')).FullName
Copy-Item -LiteralPath $Dylan -Destination (Join-Path $inputs 'dylan.exe') -Force
Copy-Item -LiteralPath $Veltrix -Destination (Join-Path $inputs 'veltrix.exe') -Force
$configName = if ($stable) { 'app-stable.json' } else { 'app.json' }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot $configName) -Destination (Join-Path $inputs 'app.json') -Force
if ((Get-FileHash -LiteralPath (Join-Path $inputs 'veltrix.exe') -Algorithm SHA256).Hash -ine $veltrixRelease.sha256) {
    throw 'La copia de Veltrix preparada para NSIS no coincide con el hash fijado'
}
Set-Content -LiteralPath (Join-Path $inputs 'version.txt') -Value $version -Encoding ascii

Push-Location $PSScriptRoot
try {
    $nsisArgs = @('/V3', "/DAPP_VERSION=$version", "/DPILOT_INPUT=$inputs", "/DOUT_DIR=$outputPath", "/DVELTRIX_FILENAME=veltrix-$($veltrixRelease.tag).exe")
    if ($stable) { $nsisArgs += '/DAPP_STABLE=1' }
    $nsisArgs += 'setup.nsi'
    & $MakeNsis @nsisArgs
    if ($LASTEXITCODE) { throw 'Falló MakeNSIS' }
} finally { Pop-Location }

$name = "Dylan-Setup-$version-x64.exe"
$setup = Get-Item (Join-Path $outputPath $name)
$app = Get-Content -Raw (Join-Path $inputs 'app.json') | ConvertFrom-Json
$manifestJson = [ordered]@{
    app_id = $app.app_id
    version = $version
    channel = $app.channel
    platform = 'windows'
    architecture = 'x86_64'
    size = $setup.Length
    url = "https://github.com/paintdev16/dylan-comedor-frontend-desktop/releases/download/v$version/$name"
    sha256 = (Get-FileHash -LiteralPath $setup.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    installed_sha256 = (Get-FileHash -LiteralPath $Dylan -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json
[System.IO.File]::WriteAllText((Join-Path $outputPath 'update-windows-x86_64.json'), $manifestJson, [System.Text.UTF8Encoding]::new($false))
