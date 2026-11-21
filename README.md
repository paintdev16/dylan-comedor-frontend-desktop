# Comedor Dylan

Workspace Rust para Dylan, su backend y el código compartido. El backend aún es un punto de entrada de ejemplo.

```powershell
cargo check --workspace --locked
cargo run -p dylan
```

La versión estable para Windows x64 se instala por usuario en `%LOCALAPPDATA%\Programs\Dylan` con **NSIS** (`apps/desktop/installer/setup.nsi`) y recibe releases del canal `stable` con **Veltrix v0.1.4** (protocolo 2).

## Instalador NSIS y actualizaciones Veltrix v0.1.4 (Windows x64)

Dylan delega la búsqueda, descarga verificada e instalación a **Veltrix v0.1.4**:

- Cargo fija `veltrix-core` a `tag = "v0.1.4"`.
- `apps/desktop/installer/veltrix-release.json` fija el tag `v0.1.4` y su SHA-256 oficial (`bfb9d79e5295bf480413e0b646e1e896a7d06475a40bf76e516f01b89f553f11`).
- Dylan utiliza el **protocolo 2** (`veltrix_core::CONFIRMED_STARTUP_PROTOCOL`) y llama a `veltrix_core::confirm_startup()` únicamente después del primer render GPUI y de inicializar el estado esencial.
- Veltrix espera hasta 30 segundos la confirmación de arranque; si Dylan falla o agota el plazo, detiene el proceso nuevo y restaura el respaldo firmado en `.updater/rollback`.
- NSIS instala `veltrix-v0.1.4.exe` junto a cualquier `veltrix.exe` previo sin sobrescribir archivos activos, usa reintentos (`MoveFileRetry`) para evitar bloqueos transitorios de Windows/Defender, conserva los datos del usuario, `.updater/app.json` y `.updater/update.lock`, y actualiza `Uninstall.exe`.

### Compilar y empaquetar en Windows

```powershell
cargo build --release --locked -p dylan
$release = Get-Content -Raw apps/desktop/installer/veltrix-release.json | ConvertFrom-Json
New-Item -Force -ItemType Directory target/veltrix-release | Out-Null
Invoke-WebRequest "https://github.com/paintdev16/veltrix/releases/download/$($release.tag)/veltrix.exe" -OutFile target/veltrix-release/veltrix.exe
./apps/desktop/installer/package-pilot.ps1 -MakeNsis "${env:ProgramFiles(x86)}\NSIS\makensis.exe" -Dylan target/release/dylan.exe -Veltrix target/veltrix-release/veltrix.exe -Output target/pilot
```

### Firma y publicación de releases

GitHub Actions compila Dylan, genera el Setup NSIS y el manifiesto `update-windows-x86_64.json`, comprueba que NSIS rechaza destinos ajenos y, cuando se ejecuta sobre una etiqueta `v*`, firma el manifiesto con Ed25519 (`DYLAN_SIGNING_KEY` + `veltrix-sign` `v0.1.4`) y prepara el release en borrador sin publicarlo automáticamente.

Para firmar y publicar manualmente desde un equipo autorizado:

```powershell
cargo install --locked --git https://github.com/paintdev16/veltrix --tag v0.1.4 --root target/veltrix-tools veltrix-sign
./apps/desktop/installer/sign-pilot.ps1 -Bundle target/pilot -Signer target/veltrix-tools/bin/veltrix-sign.exe -RawKey '<ruta privada fuera del repositorio>'
./apps/desktop/installer/publish-release.ps1 -Bundle target/pilot -VerifyOnly
./apps/desktop/installer/publish-release.ps1 -Bundle target/pilot
```
