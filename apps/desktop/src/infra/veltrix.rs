use crate::ports::updater::{AvailableUpdate, PreparedUpdate, UpdaterPort};
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use veltrix_core::{Channel, Policy, UpdateRequest, VerifiedManifest};

const OWNER: &str = "paintdev16";
const REPO: &str = "dylan-comedor-frontend-desktop";
pub const IS_BETA: bool = !env!("CARGO_PKG_VERSION_PRE").is_empty();
const APP_ID: &str = if IS_BETA {
    "com.paintdev16.dylan.veltrix-beta"
} else {
    "com.paintdev16.dylan"
};
const CHANNEL: Channel = if IS_BETA {
    Channel::Beta
} else {
    Channel::Stable
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Adaptador real de actualizaciones sobre Veltrix v0.1.4 (protocolo 2).
pub struct VeltrixUpdater;

struct VeltrixPreparedUpdate {
    job: tempfile::TempDir,
}

impl AvailableUpdate for Update {
    fn version(&self) -> &str {
        &self.version
    }

    fn download(
        &self,
        downloaded: &AtomicU64,
        total: &AtomicU64,
    ) -> std::result::Result<Box<dyn PreparedUpdate>, String> {
        let job = download(self, downloaded, total)?;
        Ok(Box::new(VeltrixPreparedUpdate { job }))
    }
}

impl PreparedUpdate for VeltrixPreparedUpdate {
    fn handoff(self: Box<Self>) -> std::result::Result<(), String> {
        handoff(self.job)
    }
}

impl UpdaterPort for VeltrixUpdater {
    fn is_installed(&self) -> bool {
        installed()
    }

    fn check(&self) -> std::result::Result<Option<Arc<dyn AvailableUpdate>>, String> {
        check().map(|maybe| maybe.map(|update| Arc::new(update) as Arc<dyn AvailableUpdate>))
    }
}

#[derive(Deserialize)]
struct Config {
    app_id: String,
    channel: Channel,
    executable: String,
    public_key: String,
    #[serde(default)]
    additional_public_keys: Vec<String>,
    max_package_size: u64,
}

#[derive(Clone)]
pub struct Update {
    pub version: String,
    manifest: VerifiedManifest,
    bytes: Vec<u8>,
    signature: Vec<u8>,
}

fn install_root() -> Result<PathBuf> {
    let path = std::env::current_exe()?;
    Ok(path
        .parent()
        .ok_or("No se encontró la carpeta de Dylan")?
        .to_owned())
}

pub fn installed() -> bool {
    install_root().is_ok_and(|root| root.join(".updater/app.json").is_file())
}

pub fn prepare_startup() -> Result<()> {
    let root = install_root()?;
    let ack = std::env::var_os(veltrix_core::STARTUP_ACK_ENV).map(PathBuf::from);
    if ack.is_none() && !installed() {
        return Ok(());
    }
    check_startup(&root, ack.as_deref())
}

fn check_startup(root: &Path, ack: Option<&Path>) -> Result<()> {
    let control = root.join(".updater");
    require_plain_path(&control, true)?;
    let lock_path = control.join("update.lock");
    require_plain_path(&lock_path, false)?;
    let lock = File::options().read(true).write(true).open(lock_path)?;
    if let Some(ack) = ack {
        // Veltrix conserva el bloqueo hasta que reciba la confirmación y salga.
        let attempt = ack.parent().ok_or("Ruta de confirmación incompleta")?;
        require_plain_path(attempt, true)?;
        if !ack.is_absolute()
            || ack.file_name().and_then(|name| name.to_str()) != Some("ready")
            || !attempt
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("startup-"))
            || attempt.parent() != Some(control.as_path())
            || attempt.canonicalize()?.parent() != Some(control.canonicalize()?.as_path())
            || !matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock))
        {
            return Err("El reinicio no corresponde a una actualización activa de Veltrix".into());
        }
        installed_policy(root)?;
    } else {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    if fs::read_to_string(root.join("version.txt"))?.trim() != env!("CARGO_PKG_VERSION") {
        return Err("La versión del ejecutable no coincide con la instalación".into());
    }
    Ok(())
}

pub fn check() -> std::result::Result<Option<Update>, String> {
    find_update().map_err(|e| e.to_string())
}

fn find_update() -> Result<Option<Update>> {
    let root = install_root()?;
    let (policy, keys) = installed_policy(&root)?;
    let client = veltrix_core::http_client()?;
    let releases = veltrix_core::github_releases(&client, OWNER, REPO)?;
    let Some(release) =
        veltrix_core::select_release(&releases, &policy.current_version, policy.channel)
    else {
        return Ok(None);
    };
    let (manifest, bytes, signature) = veltrix_core::release_manifest_with_keys(
        &client, &policy, release, &keys,
    )
    .map_err(|error| {
        format!(
            "El release {} no es válido para este piloto: {error}",
            release.tag_name
        )
    })?;
    let description = manifest.manifest();
    let version = description.version.to_string();
    let expected = format!(
        "https://github.com/{OWNER}/{REPO}/releases/download/{}/Dylan-Setup-{version}-x64.exe",
        release.tag_name
    );
    if description.url != expected
        || !release
            .assets
            .iter()
            .any(|asset| asset.browser_download_url == expected)
    {
        return Err("El instalador firmado no coincide con el asset del release de Dylan".into());
    }
    if !description
        .installed_sha256
        .as_ref()
        .is_some_and(|hash| decode_key(hash).is_ok())
    {
        return Err("Falta el SHA-256 firmado del ejecutable instalado".into());
    }
    Ok(Some(Update {
        version,
        manifest,
        bytes,
        signature,
    }))
}

fn installed_policy(root: &Path) -> Result<(Policy, Vec<[u8; 32]>)> {
    let config: Config = serde_json::from_slice(&fs::read(root.join(".updater/app.json"))?)?;
    let version = fs::read_to_string(root.join("version.txt"))?;
    pilot_policy(config, version.trim())
}

fn pilot_policy(config: Config, version: &str) -> Result<(Policy, Vec<[u8; 32]>)> {
    if config.app_id != APP_ID || config.executable != "dylan.exe" || config.channel != CHANNEL {
        return Err("Configuración de confianza de Dylan inválida".into());
    }
    if version != env!("CARGO_PKG_VERSION") {
        return Err("La versión del ejecutable no coincide con la instalación".into());
    }
    if config.additional_public_keys.len() > 8 {
        return Err("Demasiadas claves públicas de actualización".into());
    }
    let keys = config
        .additional_public_keys
        .iter()
        .map(|key| decode_key(key))
        .collect::<Result<Vec<_>>>()?;
    let policy = Policy {
        app_id: config.app_id,
        channel: config.channel,
        current_version: version.parse()?,
        platform: "windows".into(),
        architecture: std::env::consts::ARCH.into(),
        public_key: decode_key(&config.public_key)?,
        max_package_size: config.max_package_size,
    };
    Ok((policy, keys))
}

fn decode_key(key: &str) -> Result<[u8; 32]> {
    hex::decode(key)?
        .try_into()
        .map_err(|_| "Se esperaban 32 bytes hexadecimales".into())
}

pub fn download(
    update: &Update,
    downloaded: &AtomicU64,
    total: &AtomicU64,
) -> std::result::Result<tempfile::TempDir, String> {
    let run = || -> Result<_> {
        let jobs = install_root()?.join(".updater/jobs");
        fs::create_dir_all(&jobs)?;
        let job = tempfile::Builder::new().prefix("job-").tempdir_in(jobs)?;
        fs::write(job.path().join("manifest.json"), &update.bytes)?;
        fs::write(job.path().join("manifest.sig"), &update.signature)?;
        let client = veltrix_core::http_client()?;
        total.store(update.manifest.manifest().size, Ordering::Relaxed);
        veltrix_core::download(
            &client,
            &update.manifest,
            &job.path().join("package.exe"),
            &AtomicBool::new(false),
            |done, _| {
                downloaded.store(done, Ordering::Relaxed);
            },
        )?;
        Ok(job)
    };
    run().map_err(|e| e.to_string())
}

pub fn handoff(job: tempfile::TempDir) -> std::result::Result<(), String> {
    let run = || -> Result<()> {
        let root = install_root()?;
        let control = root.join(".updater");
        let pin: VeltrixPin =
            serde_json::from_str(include_str!("../../installer/veltrix-release.json"))?;
        let updater = resolve_updater_executable(&control, &pin)?;
        prepare_rollback(&root, &control)
            .map_err(|error| format!("No se pudo preparar un respaldo firmado: {error}"))?;
        let name = job
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Directorio de actualización inválido")?;
        fs::write(control.join("last-job.txt"), name)?;
        let mut child = UpdateRequest {
            protocol: veltrix_core::CONFIRMED_STARTUP_PROTOCOL,
            app_id: APP_ID.into(),
            parent_pid: std::process::id(),
            job_dir: job.keep(),
        }
        .launch(&updater)?;
        veltrix_core::wait_for_ready(&mut child)?;
        Ok(())
    };
    run().map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct VeltrixPin {
    tag: String,
    sha256: String,
}

fn resolve_updater_executable(control: &Path, pin: &VeltrixPin) -> Result<PathBuf> {
    let candidates = [
        control.join(format!("veltrix-{}.exe", pin.tag)),
        control.join("veltrix.exe"),
    ];
    let mut last_error: Option<String> = None;
    for candidate in candidates {
        if let Err(error) = require_plain_path(&candidate, false) {
            if error.kind() != io::ErrorKind::NotFound {
                last_error = Some(error.to_string());
            }
            continue;
        }
        match veltrix_core::verify_file_sha256(&candidate, &pin.sha256) {
            Ok(()) => return Ok(candidate),
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    let detail = last_error.unwrap_or_else(|| "no se encontró el ejecutable oficial".into());
    Err(format!("El ejecutable de Veltrix no es válido: {detail}").into())
}

fn rollback_policy(root: &Path) -> Result<(Policy, Vec<[u8; 32]>, String)> {
    let (mut policy, keys) = installed_policy(root)?;
    let current = policy.current_version.to_string();
    // Veltrix exige que el manifiesto supere la versión de la política.
    // El respaldo se limita además a la versión actualmente instalada.
    policy.current_version = "0.0.0-0".parse()?;
    Ok((policy, keys, current))
}

fn check_rollback_manifest(manifest: &VerifiedManifest, root: &Path, current: &str) -> Result<()> {
    let manifest = manifest.manifest();
    if manifest.version.to_string() != current {
        return Err("El respaldo no corresponde a la versión instalada".into());
    }
    let hash = manifest
        .installed_sha256
        .as_deref()
        .ok_or("El respaldo no especifica el SHA-256 de dylan.exe")?;
    veltrix_core::verify_file_sha256(&root.join("dylan.exe"), hash)?;
    Ok(())
}

fn read_rollback_file(path: &Path, max: u64) -> Result<Vec<u8>> {
    require_plain_path(path, false)?;
    let mut data = Vec::new();
    File::open(path)?.take(max + 1).read_to_end(&mut data)?;
    if data.len() as u64 > max {
        return Err("El archivo del respaldo supera el tamaño permitido".into());
    }
    Ok(data)
}

fn require_plain_path(path: &Path, directory: bool) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    let reparse_point = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse_point = metadata.is_symlink();
    if reparse_point
        || !(if directory {
            metadata.is_dir()
        } else {
            metadata.is_file()
        })
    {
        return Err(io::Error::other("Ruta de Veltrix inválida"));
    }
    Ok(())
}

fn prepare_rollback(root: &Path, control: &Path) -> Result<()> {
    // Serializar la preparación con Veltrix. El bloqueo se libera antes de lanzarlo.
    require_plain_path(control, true)?;
    let lock_path = control.join("update.lock");
    require_plain_path(&lock_path, false)?;
    let lock = File::options().read(true).write(true).open(lock_path)?;
    lock.try_lock()?;
    let (policy, keys, current) = rollback_policy(root)?;
    let backup = control.join("rollback");
    let validate_cached = || -> Result<()> {
        require_plain_path(&backup, true)?;
        let bytes = read_rollback_file(&backup.join("manifest.json"), 1024 * 1024)?;
        let signature = read_rollback_file(&backup.join("manifest.sig"), 64)?;
        let verified = policy.verify_manifest_with_keys(&bytes, &signature, &keys)?;
        check_rollback_manifest(&verified, root, &current)?;
        let package = backup.join("package.exe");
        require_plain_path(&package, false)?;
        veltrix_core::verify_package(&package, &verified)?;
        Ok(())
    };
    if validate_cached().is_ok() {
        return Ok(());
    }

    let client = veltrix_core::http_client()?;
    let releases = veltrix_core::github_releases(&client, OWNER, REPO)?;
    let tag = format!("v{current}");
    let release = releases
        .iter()
        .find(|release| !release.draft && release.tag_name == tag && release.prerelease == IS_BETA)
        .ok_or("La versión instalada no tiene un release firmado para restaurar")?;
    let (verified, bytes, signature) =
        veltrix_core::release_manifest_with_keys(&client, &policy, release, &keys)?;
    check_rollback_manifest(&verified, root, &current)?;
    let url = format!(
        "https://github.com/{OWNER}/{REPO}/releases/download/{tag}/Dylan-Setup-{current}-x64.exe"
    );
    if verified.manifest().url != url
        || !release
            .assets
            .iter()
            .any(|asset| asset.browser_download_url == url)
    {
        return Err("El instalador de respaldo no corresponde al release de Dylan".into());
    }
    let transaction = tempfile::Builder::new()
        .prefix("rollback-")
        .tempdir_in(control)?;
    let candidate = transaction.path().join("candidate");
    fs::create_dir(&candidate)?;
    fs::write(candidate.join("manifest.json"), bytes)?;
    fs::write(candidate.join("manifest.sig"), signature)?;
    veltrix_core::download(
        &client,
        &verified,
        &candidate.join("package.exe"),
        &AtomicBool::new(false),
        |_, _| {},
    )?;
    replace_rollback(transaction, &candidate, &backup)?;
    Ok(())
}

fn replace_rollback(
    transaction: tempfile::TempDir,
    candidate: &Path,
    backup: &Path,
) -> io::Result<()> {
    let previous = transaction.path().join("previous");
    let had_previous = match fs::rename(backup, &previous) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    if let Err(error) = fs::rename(candidate, backup) {
        if had_previous && let Err(restore) = fs::rename(&previous, backup) {
            let recovery = transaction.keep();
            return Err(io::Error::other(format!(
                "{error}. Respaldo anterior guardado en {}: {restore}",
                recovery.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
fn previous_result_in(control: &Path) -> Option<veltrix_core::UpdateOutcome> {
    let name = fs::read_to_string(control.join("last-job.txt")).ok()?;
    if !name.starts_with("job-") || Path::new(&name).file_name()?.to_str()? != name {
        return None;
    }
    let path = control.join("jobs").join(name);
    Some(match veltrix_core::read_update_events(&path) {
        Ok(events) => veltrix_core::update_outcome(&events),
        Err(_) => veltrix_core::UpdateOutcome::InProgress,
    })
}

pub fn record_window_started() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let control = exe.with_file_name(".updater");
    if control.is_dir() {
        fs::write(
            control.join("started.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
            }))?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veltrix_core::UpdateOutcome;

    const CONFIG: &str = if IS_BETA {
        include_str!("../../installer/app.json")
    } else {
        include_str!("../../installer/app-stable.json")
    };
    const VERSION: &str = env!("CARGO_PKG_VERSION");

    #[test]
    fn accepts_the_packaged_configuration() {
        let config = serde_json::from_str(CONFIG).unwrap();
        let (policy, _) = pilot_policy(config, VERSION).unwrap();
        assert_eq!(policy.app_id, APP_ID);
        assert_eq!(policy.channel, CHANNEL);
    }

    #[test]
    fn rejects_an_old_pilot_identity_or_a_stale_version_marker() {
        let mut config: Config = serde_json::from_str(CONFIG).unwrap();
        config.app_id = "com.paintdev16.dylan.pilot".into();
        assert!(pilot_policy(config, VERSION).is_err());
        let config = serde_json::from_str(CONFIG).unwrap();
        assert!(pilot_policy(config, "0.0.0").is_err());
    }

    #[test]
    fn confirmed_restart_can_initialize_while_veltrix_holds_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join(".updater");
        let attempt = control.join("startup-test");
        fs::create_dir_all(&attempt).unwrap();
        fs::write(control.join("app.json"), CONFIG).unwrap();
        fs::write(root.path().join("version.txt"), VERSION).unwrap();
        let lock = File::create(control.join("update.lock")).unwrap();
        let ack = attempt.join("ready");

        assert!(check_startup(root.path(), Some(&ack)).is_err());
        lock.try_lock().unwrap();
        assert!(check_startup(root.path(), Some(&ack)).is_ok());
        assert!(
            !ack.exists(),
            "Inicializar el estado no debe confirmar la ventana"
        );
        assert!(check_startup(root.path(), Some(&attempt.join("unexpected"))).is_err());
        fs::write(root.path().join("version.txt"), "0.0.0").unwrap();
        assert!(check_startup(root.path(), Some(&ack)).is_err());
    }

    #[test]
    fn unsuccessful_backup_replacement_preserves_the_previous_backup() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("rollback");
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join("manifest.json"), b"previous").unwrap();
        let transaction = tempfile::tempdir_in(root.path()).unwrap();
        let missing = transaction.path().join("missing");
        assert!(replace_rollback(transaction, &missing, &backup).is_err());
        assert_eq!(fs::read(backup.join("manifest.json")).unwrap(), b"previous");
    }

    #[test]
    fn pinned_veltrix_release_matches_v0_1_4_and_valid_sha256() {
        let pin: VeltrixPin =
            serde_json::from_str(include_str!("../../installer/veltrix-release.json")).unwrap();
        assert_eq!(pin.tag, "v0.1.4");
        assert!(decode_key(&pin.sha256).is_ok());
    }

    #[test]
    fn update_request_uses_confirmed_startup_protocol() {
        let request = UpdateRequest {
            protocol: veltrix_core::CONFIRMED_STARTUP_PROTOCOL,
            app_id: APP_ID.into(),
            parent_pid: 1234,
            job_dir: PathBuf::from("C:\\temp\\job-1"),
        };
        let json: serde_json::Value = serde_json::to_value(&request).unwrap();
        assert_eq!(json["protocol"], 2);
        assert_eq!(json["app_id"], APP_ID);
    }

    #[test]
    fn ordinary_startup_checks_lock_and_version_and_rejects_invalid_ack_paths() {
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join(".updater");
        let wrong_dir = control.join("other-dir");
        fs::create_dir_all(&wrong_dir).unwrap();
        fs::write(control.join("app.json"), CONFIG).unwrap();
        fs::write(root.path().join("version.txt"), VERSION).unwrap();
        let lock = File::create(control.join("update.lock")).unwrap();

        assert!(check_startup(root.path(), None).is_ok());
        assert!(check_startup(root.path(), Some(Path::new("startup-rel/ready"))).is_err());

        lock.try_lock().unwrap();
        assert!(check_startup(root.path(), Some(&wrong_dir.join("ready"))).is_err());
        assert!(check_startup(root.path(), Some(&root.path().join("ready"))).is_err());
        drop(lock);

        fs::write(root.path().join("version.txt"), "0.0.0").unwrap();
        assert!(check_startup(root.path(), None).is_err());
    }

    #[test]
    fn previous_result_tracks_in_progress_succeeded_and_failed_outcomes() {
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join(".updater");
        let job = control.join("jobs").join("job-test");
        fs::create_dir_all(&job).unwrap();

        assert_eq!(previous_result_in(&control), None);

        fs::write(control.join("last-job.txt"), "../escape").unwrap();
        assert_eq!(previous_result_in(&control), None);

        fs::write(control.join("last-job.txt"), "job-test").unwrap();
        assert_eq!(
            previous_result_in(&control),
            Some(UpdateOutcome::InProgress)
        );

        fs::write(
            job.join("events.jsonl"),
            "{\"stage\":\"Reiniciando aplicación\",\"message\":\"Esperando confirmación\",\"terminal\":false,\"success\":false}\n",
        )
        .unwrap();
        assert_eq!(
            previous_result_in(&control),
            Some(UpdateOutcome::InProgress)
        );

        fs::write(
            job.join("events.jsonl"),
            "{\"stage\":\"Actualización fallida\",\"message\":\"Tiempo agotado y rollback aplicado\",\"terminal\":true,\"success\":false}\n",
        )
        .unwrap();
        assert_eq!(previous_result_in(&control), Some(UpdateOutcome::Failed));

        fs::write(
            job.join("events.jsonl"),
            "{\"stage\":\"Actualización completada\",\"message\":\"Arranque confirmado\",\"terminal\":true,\"success\":true}\n",
        )
        .unwrap();
        assert_eq!(previous_result_in(&control), Some(UpdateOutcome::Succeeded));
    }

    #[test]
    fn resolve_updater_prefers_versioned_binary_and_falls_back_to_veltrix_exe() {
        let root = tempfile::tempdir().unwrap();
        let control = root.path().join(".updater");
        fs::create_dir_all(&control).unwrap();
        let pin = VeltrixPin {
            tag: "v0.1.4".into(),
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
        };
        let payload = b"hello";

        assert!(resolve_updater_executable(&control, &pin).is_err());

        let generic = control.join("veltrix.exe");
        fs::write(&generic, payload).unwrap();
        assert_eq!(resolve_updater_executable(&control, &pin).unwrap(), generic);

        fs::write(&generic, b"older-veltrix").unwrap();
        assert!(resolve_updater_executable(&control, &pin).is_err());

        let versioned = control.join("veltrix-v0.1.4.exe");
        fs::write(&versioned, payload).unwrap();
        assert_eq!(
            resolve_updater_executable(&control, &pin).unwrap(),
            versioned
        );
    }

    #[test]
    fn nsis_script_preserves_retries_atomic_uninstaller_logging_and_rollback() {
        let script = include_str!("../../installer/setup.nsi");
        for required in [
            "Function LogSetup",
            "$INSTDIR\\.updater\\setup.log",
            "Function CopyFileRetry",
            "Function MoveFileRetry",
            "WriteUninstaller \"$NewUninstaller\"",
            "event=warning op=create_shortcut_dir",
            "event=warning op=create_shortcut",
            "event=restore_start failed_op=$FailedOp win32=$FailedWinError",
            "event=failed op=$FailedOp win32=$LastWinError exit=22",
            "SetErrorLevel 20",
            "SetErrorLevel 22",
            "SetErrorLevel 23",
        ] {
            assert!(
                script.contains(required),
                "Falta elemento requerido en setup.nsi: {required}"
            );
        }
        assert!(
            !script.contains("StrCpy $FailedOp \"create_shortcut\""),
            "Los fallos de acceso directo no deben provocar rollback"
        );
    }
}
