//! Tiny JSON persistence with atomic writes (same files and format as the Electron client).

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

pub struct JsonStore<T> {
    file: PathBuf,
    value: T,
    dirty: bool,
}

impl<T: Serialize + DeserializeOwned + Default> JsonStore<T> {
    pub fn open(file: PathBuf) -> JsonStore<T> {
        let value = match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<T>(&text) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Could not read {}, keeping a copy and starting fresh: {e}", file.display());
                    let _ = std::fs::copy(&file, format!("{}.corrupt-{}", file.display(), crate::util::now_ms()));
                    T::default()
                }
            },
            Err(_) => T::default(),
        };
        JsonStore { file, value, dirty: false }
    }

    pub fn data(&self) -> &T {
        &self.value
    }

    /// Change the value; it is written by the next `flush` (soon) or `save_now`.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut T) -> R) -> R {
        let r = f(&mut self.value);
        self.dirty = true;
        r
    }

    pub fn save_now(&mut self) {
        self.dirty = false;
        if let Err(e) = write_atomic(&self.file, &self.value) {
            eprintln!("Could not write {}: {e}", self.file.display());
        }
    }

    pub fn flush(&mut self) {
        if self.dirty {
            self.save_now();
        }
    }

    /// Like `flush`, but writes `f(value)`: the value in memory stays complete.
    pub fn flush_as<U: Serialize>(&mut self, f: impl FnOnce(&T) -> U) {
        if self.dirty {
            self.dirty = false;
            if let Err(e) = write_atomic(&self.file, &f(&self.value)) {
                eprintln!("Could not write {}: {e}", self.file.display());
            }
        }
    }
}

pub fn write_atomic<T: Serialize>(file: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", file.display()));
    std::fs::write(&tmp, serde_json::to_string_pretty(value).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, file)
}
