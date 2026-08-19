use eframe::Storage;
use std::path::{Path, PathBuf};

const KEY_DOWNLOAD_PATH: &str = "download_path";

pub fn default_download_path() -> PathBuf {
    std::env::temp_dir().join("gmod-wstool")
}

pub fn load_download_path(storage: &dyn Storage) -> PathBuf {
    storage
        .get_string(KEY_DOWNLOAD_PATH)
        .map(PathBuf::from)
        .unwrap_or_else(default_download_path)
}

pub fn save_download_path(storage: &mut dyn Storage, path: &Path) {
    storage.set_string(KEY_DOWNLOAD_PATH, path.to_string_lossy().into_owned());
}
