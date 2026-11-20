param(
    [Parameter(Mandatory)][string]$Bundle,
    [switch]$VerifyOnly,
    [switch]$DraftOnly,
    [string]$CargoManifest,
    [long]$DraftId
)

$ErrorActionPreference = 'Stop'
$repo = 'paintdev16/dylan-comedor-frontend-desktop'
$api = "https://api.github.com/repos/$repo"
$cargoManifest = if ($CargoManifest) { (Resolve-Path -LiteralPath $CargoManifest).Path } else { Join-Path $PSScriptRoot '../../../Cargo.toml' }
$bundlePath = (Resolve-Path -LiteralPath $Bundle).Path
$metadata = cargo metadata --manifest-path $cargoManifest --locked --no-deps --format-version 1 | ConvertFrom-Json
if ($LASTEXITCODE) { throw 'No se pudo consultar la versión de Cargo' }
$version = ($metadata.packages | Where-Object name -ceq 'dylan').version
if ($version -notmatch '^(?:[0-9]+\.[0-9]+\.[0-9]+|1\.0\.0-beta\.[1-9][0-9]*)$') { throw "Versión no admitida: $version" }
$tag = "v$version"
$prerelease = $version.Contains('-')
$files = @("Dylan-Setup-$version-x64.exe", 'update-windows-x86_64.json', 'update-windows-x86_64.json.sig')
$local = @{}
foreach ($name in $files) {
    $file = Get-Item -LiteralPath (Join-Path $bundlePath $name)
    if ($file.PSIsContainer -or $file.Length -lt 1) { throw "Archivo requerido inválido: $name" }
    $local[$name] = @{
        Path = $file.FullName
        Size = $file.Length
        Digest = 'sha256:' + (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

cargo run --manifest-path $cargoManifest --locked -p dylan --bin verify-release -- $bundlePath
if ($LASTEXITCODE) { throw 'El paquete no pasó la verificación de Veltrix' }
if ($VerifyOnly) { return }

$token = $env:GH_TOKEN
if (-not $token) {
    $env:GIT_TERMINAL_PROMPT = '0'
    $env:GCM_INTERACTIVE = 'never'
    $entry = "protocol=https`nhost=github.com`npath=$repo.git`n`n" | git credential fill 2>$null
    $line = $entry | Where-Object { $_ -like 'password=*' } | Select-Object -First 1
    if ($line) { $token = $line.Substring(9) }
}
if (-not $token) { throw 'Configura GH_TOKEN o inicia sesión con Git Credential Manager' }
$headers = @{
    Authorization = "Bearer $token"
    Accept = 'application/vnd.github+json'
    'User-Agent' = 'Dylan-release'
}

function Assert-Draft($release) {
    if ($release.tag_name -cne $tag -or -not $release.draft -or $release.prerelease -ne $prerelease) {
        throw "Se esperaba un borrador para $tag con el canal correcto"
    }
}

function Assert-Asset($asset, [string]$name) {
    if (-not $asset -or $asset.name -cne $name -or $asset.state -cne 'uploaded' -or
        $asset.size -ne $local[$name].Size -or $asset.digest -cne $local[$name].Digest) {
        throw "El archivo remoto no coincide con el paquete verificado: $name"
    }
}

# Requiere un tag existente. La API no debe crear tags implícitamente.
Invoke-RestMethod "$api/git/ref/tags/$tag" -Headers $headers | Out-Null
$existing = Invoke-WebRequest "$api/releases/tags/$tag" -Headers $headers -SkipHttpErrorCheck
if ($existing.StatusCode -eq 200) {
    $release = $existing.Content | ConvertFrom-Json
    if (-not $release.draft) { throw "$tag ya es público. Este flujo solo publica borradores" }
    if (-not $DraftId) { $DraftId = $release.id }
} elseif ($existing.StatusCode -ne 404) {
    throw "No se pudo consultar $tag (HTTP $($existing.StatusCode))"
}

if ($DraftId) {
    $draft = Invoke-RestMethod "$api/releases/$DraftId" -Headers $headers
} else {
    $body = @{
        tag_name = $tag
        name = "Dylan $tag"
        draft = $true
        prerelease = $prerelease
        generate_release_notes = $true
        make_latest = 'false'
    } | ConvertTo-Json
    $draft = Invoke-RestMethod "$api/releases" -Headers $headers -Method Post -ContentType 'application/json' -Body $body
}
Assert-Draft $draft
Write-Output "Borrador $tag (ID $($draft.id))"

$filesUrl = "$api/releases/$($draft.id)/assets?per_page=100"
$assets = Invoke-RestMethod $filesUrl -Headers $headers
foreach ($name in $files) {
    $asset = $assets | Where-Object name -ceq $name | Select-Object -First 1
    if (-not $asset) {
        $upload = "https://uploads.github.com/repos/$repo/releases/$($draft.id)/assets?name=$([uri]::EscapeDataString($name))"
        $asset = Invoke-RestMethod $upload -Headers $headers -Method Post -ContentType 'application/octet-stream' -InFile $local[$name].Path
    }
    Assert-Asset $asset $name
}

# Validar lo que GitHub tiene almacenado antes de cambiar la visibilidad.
$assets = Invoke-RestMethod $filesUrl -Headers $headers
foreach ($name in $files) {
    Assert-Asset ($assets | Where-Object name -ceq $name | Select-Object -First 1) $name
}
$releaseUrl = "$api/releases/$($draft.id)"
$verifiedDraft = Invoke-RestMethod $releaseUrl -Headers $headers
Assert-Draft $verifiedDraft
if ($DraftOnly) {
    Write-Output $verifiedDraft.html_url
    return
}
$latest = if ($prerelease) { 'false' } else { 'true' }
$body = @{ draft = $false; prerelease = $prerelease; make_latest = $latest } | ConvertTo-Json
$published = Invoke-RestMethod $releaseUrl -Headers $headers -Method Patch -ContentType 'application/json' -Body $body
if ($published.draft -or $published.tag_name -cne $tag -or $published.prerelease -ne $prerelease) {
    throw 'GitHub devolvió un estado inesperado al publicar'
}
Write-Output $published.html_url
