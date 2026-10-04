use std::sync::Mutex;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

const LATEST: &str = "https://api.github.com/repos/Priax/Rouillo/releases/latest";

/// Releases are signed in CI with the matching private key (a repository
/// secret); a binary whose signature does not check is never run.
const PUBLIC_KEY: [u8; 32] = [
    60, 117, 178, 95, 255, 153, 218, 104, 144, 185, 92, 18, 241, 140, 18, 69, 122, 118, 52, 136, 247, 138, 233, 172,
    161, 54, 61, 96, 40, 66, 197, 220,
];

pub const ASSET: &str = if cfg!(windows) {
    "rouillo-windows-x86_64.exe"
} else {
    "rouillo-linux-x86_64"
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Available(String),
    Checking,
    Installing,
    Failed(String),
    Done,
}

#[derive(Clone)]
struct Release {
    version: String,
    binary: String,
    signature: String,
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
static RELEASE: Mutex<Option<Release>> = Mutex::new(None);

pub fn status() -> Status {
    STATUS.lock().map_or(Status::Idle, |s| s.clone())
}

fn set(status: Status) {
    if let Ok(mut s) = STATUS.lock() {
        *s = status;
    }
}

const NOT_PUBLISHED: &str = "la nouvelle version est en cours de publication, réessayez dans quelques minutes";

fn remember(release: &Release) {
    set(Status::Available(release.version.clone()));
    if let Ok(mut r) = RELEASE.lock() {
        *r = Some(release.clone());
    }
}

/// Looks for a newer release in the background.
pub fn check() {
    std::thread::spawn(|| {
        if let Ok(Some(release)) = latest() {
            remember(&release);
        }
    });
}

/// Looks for a newer release now and installs it: the one seen at start may
/// not have been published yet. Without GitHub, opens the download page.
pub fn update_now() {
    set(Status::Checking);
    std::thread::spawn(|| match latest() {
        Ok(Some(release)) => {
            remember(&release);
            set(Status::Installing);
            set(match run(&release) {
                Ok(()) => Status::Done,
                Err(e) => Status::Failed(e),
            });
        }
        Ok(None) => set(Status::Failed(NOT_PUBLISHED.to_owned())),
        Err(_) => {
            set(Status::Idle);
            crate::update::open_download_page();
        }
    });
}

/// Downloads, checks and installs the release found by `check`, then starts it.
pub fn install() {
    let Some(release) = RELEASE.lock().ok().and_then(|r| r.clone()) else {
        return;
    };
    set(Status::Installing);
    std::thread::spawn(move || {
        set(match run(&release) {
            Ok(()) => Status::Done,
            Err(e) => Status::Failed(e),
        });
    });
}

fn get(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    let mut req = ehttp::Request::get(url);
    req.headers.insert("User-Agent", "rouillo-updater");
    req.headers.insert("Accept", accept);
    let resp = ehttp::fetch_blocking(&req)?;
    if resp.status != 200 {
        return Err(format!("{url}: {}", resp.status));
    }
    Ok(resp.bytes)
}

fn latest() -> Result<Option<Release>, String> {
    #[derive(serde::Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    #[derive(serde::Deserialize)]
    struct Latest {
        tag_name: String,
        assets: Vec<Asset>,
    }
    let body = get(LATEST, "application/vnd.github+json")?;
    let latest: Latest = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let version = latest.tag_name.trim_start_matches('v').to_owned();
    if !newer(&version, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let url = |name: &str| {
        latest
            .assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    let (Some(binary), Some(signature)) = (url(ASSET), url(&format!("{ASSET}.sig"))) else {
        return Ok(None);
    };
    Ok(Some(Release {
        version,
        binary,
        signature,
    }))
}

fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// What a release signature covers: which file, for which version, with
/// which content. Signing all three keeps an old binary from passing as new.
pub fn manifest(asset: &str, version: &str, bytes: &[u8]) -> String {
    format!("{asset} {version} {:x}", Sha256::digest(bytes))
}

fn verify(key: &[u8; 32], manifest: &str, signature_hex: &str) -> bool {
    let hex = signature_hex.trim();
    let bytes: Option<Vec<u8>> = (hex.len() == 128)
        .then(|| {
            (0..64)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok())
                .collect()
        })
        .flatten();
    let (Some(bytes), Ok(key)) = (bytes, VerifyingKey::from_bytes(key)) else {
        return false;
    };
    let Ok(signature) = <[u8; 64]>::try_from(bytes.as_slice()).map(|b| Signature::from_bytes(&b)) else {
        return false;
    };
    key.verify_strict(manifest.as_bytes(), &signature).is_ok()
}

fn run(release: &Release) -> Result<(), String> {
    let binary = get(&release.binary, "application/octet-stream")?;
    let signature = get(&release.signature, "application/octet-stream")?;
    let signature = String::from_utf8_lossy(&signature);
    if !verify(&PUBLIC_KEY, &manifest(ASSET, &release.version, &binary), &signature) {
        return Err("signature invalide, fichier ignoré".to_owned());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let fresh = exe.with_extension("new");
    std::fs::write(&fresh, &binary).map_err(|e| format!("{}: {e}", fresh.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let replaced = self_replace::self_replace(&fresh).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&fresh);
    replaced?;
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("relance: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    fn sign(key: &SigningKey, text: &str) -> String {
        key.sign(text.as_bytes())
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.8.10", "0.8.9"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.8.9", "0.8.9"));
        assert!(!newer("0.8.8", "0.8.9"));
        assert!(!newer("0.9", "0.8.9"), "not a version");
        assert!(!newer("0.9.0-beta", "0.8.9"));
    }

    #[test]
    fn only_the_signed_file_for_the_signed_version_passes() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let binary = b"new game";
        let good = manifest(ASSET, "0.9.0", binary);
        let signature = sign(&key, &good);
        assert!(verify(&public, &good, &signature));
        assert!(!verify(&public, &manifest(ASSET, "0.9.0", b"tampered"), &signature));
        assert!(
            !verify(&public, &manifest(ASSET, "0.9.1", binary), &signature),
            "replayed as another version"
        );
        assert!(!verify(&public, &manifest("other.exe", "0.9.0", binary), &signature));
        let stranger = SigningKey::from_bytes(&[8; 32]);
        assert!(
            !verify(&public, &good, &sign(&stranger, &good)),
            "signed by someone else"
        );
        assert!(!verify(&public, &good, "zz"));
    }

    #[test]
    #[ignore = "needs the release key: RELEASE_KEY=path/to/key.pem, run with --ignored"]
    fn the_release_script_signs_what_the_game_checks() {
        let key = std::env::var("RELEASE_KEY").expect("RELEASE_KEY");
        let dir = std::env::temp_dir().join(format!("rouillo-sign-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(ASSET);
        std::fs::write(&file, b"a release binary").unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/sign-release.sh");
        let status = std::process::Command::new(script)
            .args([key.as_str(), "9.9.9", file.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        let signature = std::fs::read_to_string(dir.join(format!("{ASSET}.sig"))).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let signed = manifest(ASSET, "9.9.9", b"a release binary");
        assert!(
            verify(&PUBLIC_KEY, &signed, &signature),
            "the game would refuse this release"
        );
        assert!(!verify(
            &PUBLIC_KEY,
            &manifest(ASSET, "9.9.8", b"a release binary"),
            &signature
        ));
    }

    #[test]
    fn the_manifest_names_file_version_and_content() {
        let m = manifest("a", "1.2.3", b"");
        assert_eq!(
            m,
            "a 1.2.3 e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
