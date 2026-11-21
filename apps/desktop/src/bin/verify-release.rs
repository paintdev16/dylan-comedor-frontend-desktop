use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Component, Path},
};
use tar::{Archive, EntryType};
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

#[cfg(windows)]
fn verify_windows_installer(path: &Path) -> Result<()> {
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

#[cfg_attr(windows, allow(dead_code))]
fn verify_linux_archive(
    path: &Path,
    expected_version: &str,
    expected_installed_sha256: &str,
) -> Result<()> {
    let expected_hash_bytes = hex::decode(expected_installed_sha256)
        .map_err(|_| "installed_sha256 no es hexadecimal válido")?;
    if expected_hash_bytes.len() != 32 {
        return Err("installed_sha256 debe tener 32 bytes".into());
    }

    let mut magic = [0u8; 2];
    File::open(path)?.read_exact(&mut magic)?;
    if magic != [0x1f, 0x8b] {
        return Err("El paquete de actualización Linux no tiene cabecera gzip (.tar.gz)".into());
    }

    let file = File::open(path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    let mut saw_dylan = false;
    let mut saw_version = false;

    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type() != EntryType::Regular {
            return Err(
                "El paquete Linux contiene un enlace o entrada que no es un archivo regular".into(),
            );
        }

        let entry_path = entry.path()?.into_owned();
        let mut components = entry_path.components();
        let Some(Component::Normal(name_os)) = components.next() else {
            return Err("Ruta inválida dentro del paquete Linux".into());
        };
        if components.next().is_some() {
            return Err(format!(
                "No se permiten subdirectorios en el paquete Linux: {}",
                entry_path.display()
            )
            .into());
        }
        let Some(name) = name_os.to_str() else {
            return Err("Nombre de archivo no UTF-8 dentro del paquete Linux".into());
        };

        match name {
            "dylan" => {
                if saw_dylan {
                    return Err("Archivo duplicado en paquete Linux: dylan".into());
                }
                saw_dylan = true;
                let mut hasher = Sha256::new();
                let mut buf = [0u8; 64 * 1024];
                loop {
                    let read = entry.read(&mut buf)?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&buf[..read]);
                }
                let actual_hash = hasher.finalize();
                if actual_hash.as_slice() != expected_hash_bytes.as_slice() {
                    return Err(
                        "El SHA-256 del ejecutable dylan dentro del .tar.gz no coincide con installed_sha256"
                            .into(),
                    );
                }
            }
            "version.txt" => {
                if saw_version {
                    return Err("Archivo duplicado en paquete Linux: version.txt".into());
                }
                saw_version = true;
                let mut content = Vec::new();
                (&mut entry).take(257).read_to_end(&mut content)?;
                if content.len() > 256 {
                    return Err("version.txt dentro del paquete Linux es demasiado grande".into());
                }
                let text = std::str::from_utf8(&content)
                    .map_err(|_| "version.txt dentro del paquete Linux no es UTF-8 válido")?
                    .trim();
                if text != expected_version {
                    return Err(format!(
                        "version.txt dentro del .tar.gz contiene '{text}', se esperaba '{expected_version}'"
                    )
                    .into());
                }
            }
            other => {
                return Err(format!("Entrada no permitida en paquete Linux: {other}").into());
            }
        }
    }

    if !saw_dylan || !saw_version {
        return Err(
            "El paquete Linux debe contener exactamente los archivos dylan y version.txt".into(),
        );
    }

    Ok(())
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
    let installed_hash = manifest
        .installed_sha256
        .as_deref()
        .ok_or("Falta el SHA-256 firmado del ejecutable instalado")?;
    #[cfg(windows)]
    verify_windows_installer(&path)?;
    #[cfg(not(windows))]
    verify_linux_archive(&path, version, installed_hash)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use tar::{Builder, Header};

    fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn write_archive(path: &Path, entries: &[(&str, &[u8], EntryType)]) {
        let file = File::create(path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = Builder::new(encoder);

        for (name, data, entry_type) in entries {
            let mut header = Header::new_gnu();
            header.set_entry_type(*entry_type);
            if *entry_type == EntryType::Symlink {
                header.set_size(0);
                header.set_mode(0o777);
                header.set_cksum();
                builder
                    .append_link(&mut header, *name, "version.txt")
                    .unwrap();
            } else {
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, *name, *data).unwrap();
            }
        }

        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();
    }

    #[test]
    fn accepts_valid_linux_update_archive() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Dylan-Update-0.1.9-linux-x86_64.tar.gz");
        let binary = b"\x7fELF-dylan-binary-0.1.9";
        let hash = sha256_hex(binary);

        write_archive(
            &archive,
            &[
                ("dylan", binary, EntryType::Regular),
                ("version.txt", b"0.1.9\n", EntryType::Regular),
            ],
        );

        verify_linux_archive(&archive, "0.1.9", &hash).unwrap();
    }

    #[test]
    fn rejects_missing_or_incorrect_version_txt_in_linux_archive() {
        let dir = tempfile::tempdir().unwrap();
        let binary = b"\x7fELF-dylan-binary-0.1.9";
        let hash = sha256_hex(binary);

        let missing_version = dir.path().join("missing-version.tar.gz");
        write_archive(&missing_version, &[("dylan", binary, EntryType::Regular)]);
        assert!(verify_linux_archive(&missing_version, "0.1.9", &hash).is_err());

        let wrong_version = dir.path().join("wrong-version.tar.gz");
        write_archive(
            &wrong_version,
            &[
                ("dylan", binary, EntryType::Regular),
                ("version.txt", b"0.1.8\n", EntryType::Regular),
            ],
        );
        assert!(verify_linux_archive(&wrong_version, "0.1.9", &hash).is_err());
    }

    #[test]
    fn rejects_linux_archive_when_internal_dylan_hash_mismatches() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("mismatch-hash.tar.gz");
        let expected_hash = sha256_hex(b"\x7fELF-expected-dylan");

        write_archive(
            &archive,
            &[
                ("dylan", b"\x7fELF-tampered-dylan", EntryType::Regular),
                ("version.txt", b"0.1.9\n", EntryType::Regular),
            ],
        );

        assert!(verify_linux_archive(&archive, "0.1.9", &expected_hash).is_err());
    }

    #[test]
    fn rejects_duplicates_extra_entries_and_symlinks_in_linux_archive() {
        let dir = tempfile::tempdir().unwrap();
        let binary = b"\x7fELF-dylan-binary-0.1.9";
        let hash = sha256_hex(binary);

        let duplicate = dir.path().join("duplicate.tar.gz");
        write_archive(
            &duplicate,
            &[
                ("dylan", binary, EntryType::Regular),
                ("dylan", binary, EntryType::Regular),
                ("version.txt", b"0.1.9\n", EntryType::Regular),
            ],
        );
        assert!(verify_linux_archive(&duplicate, "0.1.9", &hash).is_err());

        let extra = dir.path().join("extra.tar.gz");
        write_archive(
            &extra,
            &[
                ("dylan", binary, EntryType::Regular),
                ("version.txt", b"0.1.9\n", EntryType::Regular),
                ("extra.txt", b"nope", EntryType::Regular),
            ],
        );
        assert!(verify_linux_archive(&extra, "0.1.9", &hash).is_err());

        let symlink = dir.path().join("symlink.tar.gz");
        write_archive(
            &symlink,
            &[
                ("dylan", binary, EntryType::Regular),
                ("version.txt", b"", EntryType::Symlink),
            ],
        );
        assert!(verify_linux_archive(&symlink, "0.1.9", &hash).is_err());
    }
}
