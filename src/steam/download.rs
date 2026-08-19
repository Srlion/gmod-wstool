use super::client;
use crate::steam::callbacks;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use steamworks::{DownloadItemResult, FileType, PublishedFileId};

#[derive(Clone)]
pub enum DownloadState {
    Queued,
    Pending,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    Done {
        gma: PathBuf,
        unpacked: Option<PathBuf>,
    },
    Error(String),
}

impl DownloadState {
    pub fn fraction(&self) -> f32 {
        match self {
            Self::Queued => 0.0,
            Self::Downloading { total: 0, .. } | Self::Pending => 0.0,
            Self::Downloading { downloaded, total } => *downloaded as f32 / *total as f32,
            Self::Done { .. } => 1.0,
            Self::Error(_) => 0.0,
        }
    }
}

pub fn parse_id(input: &str) -> Option<u64> {
    let s = input.trim();
    if let Ok(id) = s.parse::<u64>() {
        return Some(id);
    }
    s.split(['?', '&'])
        .find_map(|part| part.strip_prefix("id="))
        .and_then(|v| v.parse::<u64>().ok())
}

pub enum FinishResult {
    Installed(PathBuf),
    Failed(String),
}

pub struct DownloadFinishJob {
    rx: Receiver<FinishResult>,
    done: Option<FinishResult>,
}

impl DownloadFinishJob {
    pub fn start(id: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let item = PublishedFileId(id);

        callbacks::register_until(move |res: DownloadItemResult| {
            if res.app_id.0 != 4000 || res.published_file_id != item {
                return false;
            }
            let msg = if let Some(err) = res.error {
                FinishResult::Failed(format!("Steam download error: {err:?}"))
            } else {
                match client().ugc().item_install_info(item) {
                    Some(info) => FinishResult::Installed(PathBuf::from(info.folder)),
                    None => FinishResult::Failed(
                        "Download reported complete but no install info".into(),
                    ),
                }
            };
            let _ = tx.send(msg);
            true
        });

        Self { rx, done: None }
    }

    pub fn already_done(folder: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(FinishResult::Installed(folder));
        Self { rx, done: None }
    }

    pub fn poll(&mut self) -> Option<&FinishResult> {
        if self.done.is_none()
            && let Ok(r) = self.rx.try_recv()
        {
            self.done = Some(r);
        }
        self.done.as_ref()
    }
}

pub fn start_download(id: u64) -> Result<DownloadFinishJob, String> {
    let ugc = client().ugc();
    let item = PublishedFileId(id);

    let state = ugc.item_state(item);
    let installed = state.contains(steamworks::ItemState::INSTALLED);
    let needs_update = state.contains(steamworks::ItemState::NEEDS_UPDATE);
    if installed
        && !needs_update
        && let Some(info) = ugc.item_install_info(item)
    {
        return Ok(DownloadFinishJob::already_done(PathBuf::from(info.folder)));
    }

    if ugc.download_item(item, true) {
        Ok(DownloadFinishJob::start(id))
    } else {
        Err("Steam refused the download (item may not exist or isn't accessible)".into())
    }
}

pub enum PollResult {
    Pending,
    Downloading(DownloadState),
}

pub fn poll_progress(id: u64) -> PollResult {
    let ugc = client().ugc();
    let item = PublishedFileId(id);
    if let Some((downloaded, total)) = ugc.item_download_info(item)
        && total > 0
        && downloaded < total
    {
        return PollResult::Downloading(DownloadState::Downloading { downloaded, total });
    }
    PollResult::Pending
}

pub fn scan_existing(dest_dir: &Path) -> Vec<u64> {
    let Ok(entries) = std::fs::read_dir(dest_dir) else {
        return vec![];
    };
    let mut dirs: Vec<(u64, std::time::SystemTime)> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        // skip empty addon folders, they were never populated correctly
        .filter(|e| {
            std::fs::read_dir(e.path())
                .map(|mut r| r.next().is_some())
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().parse::<u64>().ok()?;
            let created = e
                .metadata()
                .and_then(|m| m.created())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((id, created))
        })
        .collect();
    dirs.sort_by_key(|b| std::cmp::Reverse(b.1));
    dirs.into_iter().map(|(id, _)| id).collect()
}

pub fn done_state(addon_dir: PathBuf) -> DownloadState {
    let gma = find_gma_in_dir(&addon_dir).unwrap_or_else(|| addon_dir.clone());
    let unpacked = addon_dir.join("unpacked");
    let unpacked = if unpacked.is_dir() {
        Some(unpacked)
    } else {
        Some(addon_dir.clone())
    };
    DownloadState::Done { gma, unpacked }
}

fn path_has_content(src: &Path) -> bool {
    if src.is_file() {
        return std::fs::metadata(src).map(|m| m.len() > 0).unwrap_or(false);
    }
    if src.is_dir() {
        return std::fs::read_dir(src)
            .map(|mut e| e.next().is_some())
            .unwrap_or(false);
    }
    false
}

fn copy_to_dest_from(src: &Path, id: u64, dest_dir: &Path) -> DownloadState {
    if !path_has_content(src) {
        return DownloadState::Error(format!(
            "Steam install path is empty or missing: {}",
            src.display()
        ));
    }

    let addon_dir = dest_dir.join(id.to_string());

    let already_populated = std::fs::read_dir(&addon_dir)
        .map(|mut e| e.next().is_some())
        .unwrap_or(false);

    if !already_populated {
        if let Err(e) = std::fs::create_dir_all(&addon_dir) {
            return DownloadState::Error(format!("Copy failed: {e}"));
        }
        let copied = if src.is_dir() {
            copy_dir(src, &addon_dir)
        } else if src.is_file() {
            let name = src
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(format!("{id}.gma")));
            std::fs::copy(src, addon_dir.join(name)).map(|_| ())
        } else {
            return DownloadState::Error(format!(
                "Steam install path doesn't exist: {}",
                src.display()
            ));
        };
        if let Err(e) = copied {
            return DownloadState::Error(format!("Copy failed: {e}"));
        }
    }

    let gma = find_gma_in_dir(&addon_dir);
    let unpacked = match &gma {
        Some(path) => match unpack_gma(path) {
            Ok(dir) => Some(dir),
            Err(e) => return DownloadState::Error(format!("Unpack failed: {e}")),
        },
        None => Some(addon_dir.clone()),
    };
    DownloadState::Done {
        gma: gma.unwrap_or_else(|| addon_dir.clone()),
        unpacked,
    }
}

pub fn unpack_gma(gma_path: &Path) -> Result<PathBuf, String> {
    let parent = gma_path
        .parent()
        .ok_or_else(|| "gma has no parent directory".to_string())?;
    let out_dir = parent.join("unpacked");
    if already_unpacked(&out_dir, gma_path) {
        return Ok(out_dir);
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("Create dir failed: {e}"))?;
    let bytes = std::fs::read(gma_path).map_err(|e| format!("Read failed: {e}"))?;
    let bytes = decompress_if_lzma(bytes)?;
    let entries = gma_lite::read(&bytes[..]).map_err(|e| format!("Parse failed: {e}"))?;

    let dir = Dir::open_ambient_dir(&out_dir, ambient_authority())
        .map_err(|e| format!("Open out dir failed: {e}"))?;

    for entry in &entries {
        let rel = Path::new(&entry.name);

        if let Some(p) = rel.parent()
            && !p.as_os_str().is_empty()
        {
            dir.create_dir_all(p)
                .map_err(|e| format!("Create dir failed for {}: {e}", entry.name))?;
        }

        dir.write(rel, &entry.content)
            .map_err(|e| format!("Write failed for {}: {e}", entry.name))?;
    }
    Ok(out_dir)
}

fn already_unpacked(dir: &Path, _gma_path: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut e| e.next().is_some())
        .unwrap_or(false)
}

fn find_gma_in_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| is_valid_gma(p))
}

fn copy_dir(src: &Path, dest: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

fn is_valid_gma(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|mut f| {
            let mut magic = [0u8; 4];
            std::io::Read::read_exact(&mut f, &mut magic)?;
            Ok(&magic == b"GMAD" || magic[0] == 0x5D)
        })
        .unwrap_or(false)
}

fn decompress_if_lzma(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.first() != Some(&0x5D) {
        return Ok(bytes);
    }
    let mut reader =
        lzma_rust2::LzmaReader::new_mem_limit(std::io::Cursor::new(bytes), u32::MAX, None)
            .map_err(|e| format!("LZMA init failed: {e}"))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut out)
        .map_err(|e| format!("LZMA decompress failed: {e}"))?;
    Ok(out)
}

pub struct CollectionResolveJob {
    id: u64,
    rx: Receiver<Result<Vec<u64>, String>>,
    done: Option<Result<Vec<u64>, String>>,
}

impl CollectionResolveJob {
    pub fn start(id: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let ugc = client().ugc();
        match ugc.query_items(vec![PublishedFileId(id)]) {
            Ok(query) => {
                let query = query.set_return_children(true);
                query.fetch(move |res| {
                    let result = match res {
                        Ok(r) => {
                            let is_collection = r
                                .get(0)
                                .is_some_and(|item| matches!(item.file_type, FileType::Collection));
                            let kids = if is_collection {
                                r.get_children(0).unwrap_or_default()
                            } else {
                                Vec::new()
                            };
                            Ok(kids.into_iter().map(|c| c.0).collect())
                        }
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send(result);
                });
            }
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
            }
        }
        Self { id, rx, done: None }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn poll(&mut self) -> Option<&Result<Vec<u64>, String>> {
        if self.done.is_none()
            && let Ok(r) = self.rx.try_recv()
        {
            self.done = Some(r);
        }
        self.done.as_ref()
    }
}

pub struct CopyUnpackJob {
    rx: Receiver<DownloadState>,
    done: Option<DownloadState>,
}

impl CopyUnpackJob {
    pub fn start(src: PathBuf, id: u64, dest_dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let state = copy_to_dest_from(&src, id, &dest_dir);
            let _ = tx.send(state);
        });
        Self { rx, done: None }
    }

    pub fn poll(&mut self) -> Option<&DownloadState> {
        if self.done.is_none()
            && let Ok(s) = self.rx.try_recv()
        {
            self.done = Some(s);
        }
        self.done.as_ref()
    }
}
