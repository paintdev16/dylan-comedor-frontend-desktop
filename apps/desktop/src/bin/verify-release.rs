use serde::Deserialize;
use std::{fs::File, io::Read, path::Path};
use veltrix_core::{Channel, Policy, verify_file_sha256, verify_package};

const REPO: &str = "paintdev16/dylan-comedor-frontend-desktop";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(windows)]
const PLATFORM: &str = "windows";
#[cfg(not(windows))]
const PLATFORM: &str = "linux";

#[cfg(windows)]
const EXECUTABLE_NAME: &str = "dylan.exe";
#[cfg(not(windows))]
const EXECUTABLE_NAME: &str = "dylan";

#[derive(Deserialize)]
struct Trust {
    app_id: String,
    channel: Channel,
    public_key: String,
    max_package_size: u64,
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(format!("Archivo demasiado grande: {}", path.display()).into());
    }
    Ok(bytes)
}

fn packaged_config() -> &'static str {
    #[cfg(windows)]
    {
        if env!("CARGO_PKG_VERSION_PRE").is_empty() {
            include_str!("../../installer/app-stable.json")
        } else {
            include_str!("../../installer/app.json")
        }
    }
    #[cfg(not(windows))]
    {
        if env!("CARGO_PKG_VERSION_PRE").is_empty() {
            include_str!("../../installer/app-linux-stable.json")
        } else {
            include_str!("../../installer/app-linux.json")
        }
    }
}

fn package_name(version: &str) -> String {
    #[cfg(windows)]
    {
        format!("Dylan-Setup-{version}-x64.exe")
    }
    #[cfg(not(windows))]
    {
        format!("Dylan-Update-{version}-linux-x86_64.tar.gz")
    }
}

fn verify_package_format(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let mut header = Vec::new();
        File::open(path)?
            .take(1024 * 1024)
            .read_to_end(&mut header)?;
        if !header.starts_with(b"MZ")
            || !header
                .chunks(512)
                .skip(1)
                .any(|block| block.get(4..20) == Some(b"\xef\xbe\xad\xdeNullsoftInst"))
        {
            return Err("El paquete no contiene una cabecera de instalador NSIS".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut magic = [0u8; 2];
        File::open(path)?.read_exact(&mut magic)?;
        if magic != [0x1f, 0x8b] {
            return Err(
                "El paquete de actualización Linux no tiene cabecera gzip (.tar.gz)".into(),
            );
        }
        Ok(())
    }
}

fn verify(bundle: &Path, version: &str) -> Result<()> {
    let parsed_version = version.parse()?;
    let trust: Trust = serde_json::from_str(packaged_config())?;
    let policy = Policy {
        app_id: trust.app_id,
        channel: trust.channel,
        current_version: "0.0.0-0".parse()?,
        platform: PLATFORM.into(),
        architecture: "x86_64".into(),
        public_key: hex::decode(&trust.public_key)?
            .try_into()
            .map_err(|_| "La clave pública no tiene 32 bytes")?,
        max_package_size: trust.max_package_size,
    };
    let manifest_name = format!("update-{PLATFORM}-x86_64.json");
    let sig_name = format!("{manifest_name}.sig");
    let bytes = read_bounded(&bundle.join(&manifest_name), 1024 * 1024)?;
    let signature = read_bounded(&bundle.join(&sig_name), 64)?;
    let verified = policy.verify_manifest(&bytes, &signature)?;
    let package = package_name(version);
    let expected_url =
        format!("https://github.com/{REPO}/releases/download/v{parsed_version}/{package}");
    let manifest = verified.manifest();
    if manifest.version != parsed_version || manifest.url != expected_url {
        return Err("La versión o la URL firmada no coincide con el release".into());
    }
    let path = bundle.join(package);
    verify_package(&path, &verified)?;
    verify_package_format(&path)?;
    let installed_hash = manifest
        .installed_sha256
        .as_deref()
        .ok_or("Falta el SHA-256 firmado del ejecutable instalado")?;
    let executable = bundle.join("inputs").join(EXECUTABLE_NAME);
    let executable = if executable.is_file() {
        executable
    } else {
        bundle.join(EXECUTABLE_NAME)
    };
    verify_file_sha256(&executable, installed_hash)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let bundle = args.next().ok_or("Uso: verify-release <carpeta>")?;
    if args.next().is_some() {
        return Err("Uso: verify-release <carpeta>".into());
    }
    verify(Path::new(&bundle), env!("CARGO_PKG_VERSION"))?;
    #[cfg(windows)]
    println!("Setup NSIS, manifiesto y firma verificados.");
    #[cfg(not(windows))]
    println!("Paquete Linux .tar.gz, manifiesto y firma verificados.");
    Ok(())
}
