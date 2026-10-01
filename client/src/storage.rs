#[cfg(target_arch = "wasm32")]
fn local() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

pub fn get(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        local()?.get_item(key).ok().flatten().filter(|v| !v.is_empty())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        file::store().get(key)
    }
}

pub fn set(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = local() {
        let _ = s.set_item(key, value);
    }
    #[cfg(not(target_arch = "wasm32"))]
    file::store().set(key, value);
}

pub fn remove(key: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = local() {
        let _ = s.remove_item(key);
    }
    #[cfg(not(target_arch = "wasm32"))]
    file::store().remove(key);
}

#[cfg(not(target_arch = "wasm32"))]
mod file {
    use std::collections::BTreeMap;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub struct FileStore {
        path: Option<PathBuf>,
        values: BTreeMap<String, String>,
    }

    impl FileStore {
        pub fn open(path: Option<PathBuf>) -> Self {
            let values = path
                .as_deref()
                .and_then(|p| fs::read_to_string(p).ok())
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            Self { path, values }
        }

        pub fn get(&self, key: &str) -> Option<String> {
            self.values.get(key).filter(|v| !v.is_empty()).cloned()
        }

        pub fn set(&mut self, key: &str, value: &str) {
            if self.values.get(key).map(String::as_str) != Some(value) {
                self.values.insert(key.to_owned(), value.to_owned());
                self.save();
            }
        }

        pub fn remove(&mut self, key: &str) {
            if self.values.remove(key).is_some() {
                self.save();
            }
        }

        fn save(&self) {
            let Some(path) = &self.path else { return };
            if let Err(e) = write_atomically(path, &self.values) {
                eprintln!("[storage] {}: {e}", path.display());
            }
        }
    }

    fn write_atomically(path: &Path, values: &BTreeMap<String, String>) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(values).map_err(io::Error::other)?;
        let tmp = path.with_extension("tmp");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&tmp)?.write_all(&json)?;
        fs::rename(&tmp, path)
    }

    fn default_path() -> Option<PathBuf> {
        if cfg!(test) {
            return None;
        }
        let dir = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Rouillo")
        } else if cfg!(target_os = "macos") {
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Rouillo")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
                .join("rouillo")
        };
        Some(dir.join("storage.json"))
    }

    pub fn store() -> MutexGuard<'static, FileStore> {
        static STORE: OnceLock<Mutex<FileStore>> = OnceLock::new();
        STORE
            .get_or_init(|| Mutex::new(FileStore::open(default_path())))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn temp_path(name: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!("rouillo-storage-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            dir.join("nested").join("storage.json")
        }

        #[test]
        fn values_survive_a_restart_and_removals_too() {
            let path = temp_path("restart");
            let mut store = FileStore::open(Some(path.clone()));
            store.set("token", "abc");
            store.set("best", "1200");
            store.remove("best");

            let reopened = FileStore::open(Some(path.clone()));
            assert_eq!(reopened.get("token").as_deref(), Some("abc"));
            assert_eq!(reopened.get("best"), None);
            let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        }

        #[test]
        fn a_corrupt_file_starts_empty_and_is_replaced() {
            let path = temp_path("corrupt");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "{not json").unwrap();
            let mut store = FileStore::open(Some(path.clone()));
            assert_eq!(store.get("token"), None);
            store.set("token", "abc");
            assert_eq!(FileStore::open(Some(path.clone())).get("token").as_deref(), Some("abc"));
            let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        }

        #[cfg(unix)]
        #[test]
        fn only_the_owner_can_read_the_file() {
            use std::os::unix::fs::PermissionsExt;
            let path = temp_path("mode");
            FileStore::open(Some(path.clone())).set("token", "secret");
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let _ = fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
        }

        #[test]
        fn without_a_path_values_live_in_memory_only() {
            let mut store = FileStore::open(None);
            store.set("token", "abc");
            assert_eq!(store.get("token").as_deref(), Some("abc"));
        }
    }
}
