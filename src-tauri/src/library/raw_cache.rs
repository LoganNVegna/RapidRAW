//! Disposable source cache. Logical paths and metadata always refer to originals.
use super::*;
use std::io::{Read, Write};
static COPY_LOCK: Mutex<()> = Mutex::new(());
static SPACE: LazyLock<Mutex<Option<(std::time::Instant, u64)>>> =
    LazyLock::new(|| Mutex::new(None));
static PREVIEW_SCAN: LazyLock<Mutex<Option<std::time::Instant>>> =
    LazyLock::new(|| Mutex::new(None));
static PREVIEW_USED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
type MountInventory = Option<(std::time::Instant, Vec<String>)>;
static MOUNTS: LazyLock<Mutex<MountInventory>> = LazyLock::new(|| Mutex::new(None));
const GIB: u64 = 1024 * 1024 * 1024;
const RAW_LIMIT: u64 = 20 * GIB;
const PREVIEW_LIMIT: u64 = 10 * GIB;
const RESERVE: u64 = 5 * GIB;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheLimits {
    raw_gib: u64,
    preview_gib: u64,
    reserve_gib: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StorageLayout {
    enabled: bool,
    cache_kind: String,
    sources: Vec<StorageSource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageSource {
    path: String,
    kind: String,
}
pub(super) fn seed_settings(settings: &crate::app_settings::AppSettings) -> Result<()> {
    for (key, value) in [
        (
            "storage-layout",
            serde_json::to_value(&settings.library_storage),
        ),
        (
            "cache-limits",
            serde_json::to_value(&settings.library_cache_defaults),
        ),
    ] {
        let value = value.map_err(|e| e.to_string())?;
        if !value.is_null() {
            with_db(|db| {
                db.execute(
                    "INSERT OR IGNORE INTO state(key,value) VALUES(?1,?2)",
                    params![key, value.to_string()],
                )
                .map(|_| ())
            })?;
        }
    }
    Ok(())
}
pub fn layout() -> Result<StorageLayout> {
    let value = with_db(|db| {
        db.query_row(
            "SELECT value FROM state WHERE key='storage-layout'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })?;
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}
#[tauri::command]
pub fn library_storage_layout(value: StorageLayout) -> Result<()> {
    if !["", "unknown", "ssd", "hdd", "network"].contains(&value.cache_kind.as_str())
        || value.sources.iter().any(|source| {
            source.path.is_empty()
                || !["unknown", "ssd", "hdd", "network"].contains(&source.kind.as_str())
        })
    {
        return Err("Choose valid storage types".into());
    }
    with_db(|db| {
        db.execute(
            "INSERT OR REPLACE INTO state(key,value) VALUES('storage-layout',?1)",
            [serde_json::to_string(&value).unwrap_or_default()],
        )
        .map(|_| ())
    })?;
    changed();
    Ok(())
}
fn eligible(source: &Path) -> Result<bool> {
    let layout = layout()?;
    if !layout.enabled || layout.cache_kind != "ssd" {
        return Ok(false);
    }
    let key = normalized(&source.to_string_lossy());
    let source_type = layout
        .sources
        .iter()
        .filter(|s| {
            let prefix = normalized(&s.path).trim_end_matches('/').to_string();
            key == prefix || key.starts_with(&(prefix + "/"))
        })
        .max_by_key(|s| s.path.len())
        .map(|s| s.kind.as_str());
    if !matches!(source_type, Some("hdd" | "network")) {
        return Ok(false);
    }
    let cache = cache_root()?;
    let mut inventory = MOUNTS.lock().map_err(|e| e.to_string())?;
    if inventory
        .as_ref()
        .is_none_or(|(time, _)| time.elapsed() > std::time::Duration::from_secs(60))
    {
        let mounts = sysinfo::Disks::new_with_refreshed_list()
            .iter()
            .map(|disk| normalized(&disk.mount_point().to_string_lossy()))
            .collect();
        *inventory = Some((std::time::Instant::now(), mounts));
    }
    let mounts = &inventory.as_ref().ok_or("No storage inventory")?.1;
    let mount = |path: &Path| {
        let path = normalized(&path.to_string_lossy());
        mounts
            .iter()
            .filter(|root| path.starts_with(root.as_str()))
            .max_by_key(|root| root.len())
    };
    // Explicit source tags also support network shares absent from the disk inventory.
    match (mount(source), mount(&cache)) {
        (Some(a), Some(b)) => Ok(a != b),
        _ => Ok(!key.starts_with(
            &(normalized(&cache.to_string_lossy())
                .trim_end_matches('/')
                .to_string()
                + "/"),
        )),
    }
}
fn limits() -> Result<CacheLimits> {
    let value = with_db(|db| {
        db.query_row(
            "SELECT value FROM state WHERE key='cache-limits'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })?;
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(CacheLimits {
            raw_gib: RAW_LIMIT / GIB,
            preview_gib: PREVIEW_LIMIT / GIB,
            reserve_gib: RESERVE / GIB,
        }))
}
#[tauri::command]
pub fn library_cache_limits(value: CacheLimits) -> Result<()> {
    if value.raw_gib > 1_048_576
        || value.preview_gib == 0
        || value.preview_gib > 1_048_576
        || value.reserve_gib > 1_048_576
    {
        return Err("Choose valid cache limits in GiB".into());
    }
    with_db(|db| {
        db.execute(
            "INSERT OR REPLACE INTO state(key,value) VALUES('cache-limits',?1)",
            [serde_json::to_string(&value).unwrap_or_default()],
        )
        .map(|_| ())
    })?;
    *PREVIEW_SCAN.lock().map_err(|e| e.to_string())? = None;
    changed();
    Ok(())
}
fn allocated(bytes: u64) {
    if let Ok(mut cached) = SPACE.lock()
        && let Some((_, space)) = cached.as_mut()
    {
        *space = space.saturating_sub(bytes);
    }
}
fn folder() -> Result<PathBuf> {
    let p = cache_root()?.join("RAW");
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    Ok(p)
}
pub(super) fn clean_interrupted_copies() -> Result<()> {
    let directory = folder()?;
    for item in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        if !item.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        else {
            continue;
        };
        let temporary = path.extension().is_some_and(|s| s == "partial");
        let orphan = path.extension().is_some_and(|s| s == "raw")
            && with_db(|db| {
                db.query_row(
                    "SELECT count(*) FROM raw_cache WHERE asset_id=?1",
                    [id],
                    |r| r.get::<_, i64>(0),
                )
            })? == 0;
        if temporary || orphan {
            owned_remove(&path)?;
        }
    }
    Ok(())
}
fn free_space() -> Result<u64> {
    let mut cached = SPACE.lock().map_err(|e| e.to_string())?;
    if let Some((time, space)) = *cached
        && time.elapsed() < std::time::Duration::from_secs(2)
    {
        return Ok(space);
    }
    let path = cache_root()?;
    let space = sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .ok_or("Cannot determine cache drive free space")?;
    *cached = Some((std::time::Instant::now(), space));
    Ok(space)
}
pub fn stats() -> Result<Value> {
    let limits = limits()?;
    let (used, pinned) = with_db(|db| {
        db.query_row("SELECT coalesce(sum(size),0),coalesce(sum(CASE WHEN pinned=1 THEN size ELSE 0 END),0) FROM raw_cache",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?)))
    })?;
    Ok(json!({
    "used":used,"pinned":pinned,"previewUsed":PREVIEW_USED.load(std::sync::atomic::Ordering::Relaxed),"rawLimit":limits.raw_gib*GIB,"previewLimit":limits.preview_gib*GIB,"reserve":limits.reserve_gib*GIB,"limits":limits,"free":free_space()?,"directory":cache_root()?.to_string_lossy()
    }
    ))
}
fn owned_remove(path: &Path) -> Result<()> {
    // Only files directly within our managed RAW directory may be removed.
    if path.parent() != Some(folder()?.as_path()) {
        return Err("Refusing to remove a file outside the managed RAW cache".into());
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn ensure_capacity(bytes: u64, raw: bool) -> Result<()> {
    let limits = limits()?;
    if raw {
        loop {
            let used = with_db(|db| {
                db.query_row("SELECT coalesce(sum(size),0) FROM raw_cache", [], |r| {
                    r.get::<_, i64>(0)
                })
            })?;
            if (used.max(0) as u64).saturating_add(bytes) <= limits.raw_gib * GIB
                && free_space()?.saturating_sub(bytes) >= limits.reserve_gib * GIB
            {
                break;
            }
            let victim = with_db(|db| {
                db.query_row("SELECT asset_id,local_path FROM raw_cache WHERE pinned=0 ORDER BY used LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()
            })?;
            let Some((id, path)) = victim else {
                return Err("Cache budget or free-space reserve reached. Unpin cached photos or free space before resuming.".into());
            };
            owned_remove(Path::new(&path))?;
            with_db(|db| {
                db.execute("DELETE FROM raw_cache WHERE asset_id=?1", [id])
                    .map(|_| ())
            })?;
        }
    } else {
        let mut last = PREVIEW_SCAN.lock().map_err(|e| e.to_string())?;
        if last.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(10)) {
            let root = cache_root()?;
            let raw = folder()?;
            let mut files = vec![];
            let analysis_size = || {
                [
                    "analysis.sqlite",
                    "analysis.sqlite-wal",
                    "analysis.sqlite-shm",
                ]
                .iter()
                .filter_map(|name| fs::metadata(root.join(name)).ok())
                .map(|m| m.len())
                .sum::<u64>()
            };
            let mut analysis = analysis_size();
            if analysis.saturating_add(bytes) > limits.preview_gib * GIB {
                with_db(|db| {
                    db.execute_batch("DELETE FROM disposable.analysis;PRAGMA disposable.wal_checkpoint(TRUNCATE);VACUUM disposable;")
                })?;
                analysis = analysis_size();
            }
            let mut used = analysis;
            // Only regenerable files inside named cache subdirectories are candidates.
            for entry in walkdir::WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    e.path() != raw
                        && (e.depth() == 0
                            || matches!(
                                e.path()
                                    .strip_prefix(&root)
                                    .ok()
                                    .and_then(|p| p.components().next())
                                    .and_then(|c| c.as_os_str().to_str()),
                                Some("thumbnails" | "exif" | "ai_maps" | "ai_denoise")
                            ))
                })
                .filter_map(std::result::Result::ok)
            {
                if !entry.file_type().is_file() || entry.path().parent() == Some(root.as_path()) {
                    continue;
                }
                let metadata = entry.metadata().map_err(|e| e.to_string())?;
                used += metadata.len();
                files.push((
                    metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
                    entry.path().to_path_buf(),
                    metadata.len(),
                ));
            }
            files.sort_by_key(|f| f.0);
            for (_, path, size) in files {
                if used.saturating_add(bytes) <= limits.preview_gib * GIB {
                    break;
                }
                fs::remove_file(path).map_err(|e| e.to_string())?;
                used = used.saturating_sub(size);
            }
            PREVIEW_USED.store(used, std::sync::atomic::Ordering::Relaxed);
            *last = Some(std::time::Instant::now());
        }
        if free_space()?.saturating_sub(bytes) < limits.reserve_gib * GIB {
            return Err(format!(
                "Preview job paused to preserve {} GiB free on the cache drive.",
                limits.reserve_gib
            ));
        }
    }
    Ok(())
}
fn hash_file(path: &Path) -> Result<String> {
    let mut input = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}
pub fn stage(path: &str, pin: bool) -> Result<()> {
    stage_cancellable(path, pin, &|| false)
}
pub fn stage_cancellable(path: &str, pin: bool, cancel: &dyn Fn() -> bool) -> Result<()> {
    let _guard = COPY_LOCK.lock().map_err(|e| e.to_string())?;
    let source = ImageKey::parse(path).source;
    let logical = source.to_string_lossy();
    index_path(&logical, "")?;
    let stamp = revision(&source).map_err(|e| e.to_string())?;
    let id = with_db(|db| {
        db.query_row(
            "SELECT id FROM assets WHERE path_key=?1",
            [normalized(&logical)],
            |r| r.get::<_, String>(0),
        )
    })?;
    if pin {
        with_db(|db| {
            db.execute("INSERT OR IGNORE INTO pins(asset_id) VALUES(?1)", [&id])
                .map(|_| ())
        })?;
    }
    let pin = pin
        || with_db(|db| {
            db.query_row("SELECT count(*) FROM pins WHERE asset_id=?1", [&id], |r| {
                r.get::<_, i64>(0)
            })
        })? > 0;
    if resolve(&source).is_some() {
        if pin {
            with_db(|db| {
                db.execute("UPDATE raw_cache SET pinned=1 WHERE asset_id=?1", [id])
                    .map(|_| ())
            })?;
        }
        return Ok(());
    }
    let size = fs::metadata(&source).map_err(|e| e.to_string())?.len();
    ensure_capacity(size, true)?;
    let destination = folder()?.join(format!("{id}.raw"));
    let temporary = folder()?.join(format!("{id}.partial"));
    let mut input = fs::File::open(&source).map_err(|e| e.to_string())?;
    let mut output = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0; 4 * 1024 * 1024];
    loop {
        if cancel() {
            return Err(
                "Cache copy interrupted; temporary bytes will be replaced on resume".into(),
            );
        }
        while super::foreground_busy() {
            if cancel() {
                return Err("Cache copy paused".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        allocated(n as u64);
        hasher.update(&buffer[..n]);
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    let hash = hasher.finalize().to_hex().to_string();
    if revision(&source).ok().as_deref() != Some(&stamp) || hash_file(&temporary)? != hash {
        let _ = owned_remove(&temporary);
        return Err("Original changed during copying or cache verification failed".into());
    }
    if destination.exists() {
        owned_remove(&destination)?;
    }
    fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    with_db(|db| {
        db.execute("INSERT OR REPLACE INTO raw_cache(asset_id,path,revision,local_path,hash,size,pinned,used) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,logical,stamp,destination.to_string_lossy(),hash,size as i64,pin,now()]).map(|_|())
    })?;
    changed();
    Ok(())
}
pub fn resolve(source: &Path) -> Option<PathBuf> {
    let item = with_db(|db| {
        db.query_row(
            "SELECT r.asset_id,r.revision,r.local_path,r.hash,r.size FROM raw_cache r JOIN assets a ON a.id=r.asset_id WHERE a.path_key=?1",
            [normalized(&source.to_string_lossy())],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
}
,
        )
        .optional()
}
)
    .ok()
    .flatten()?;
    let (id, stamp, path, hash, size) = item;
    let local = PathBuf::from(path);
    // A cache never makes an unavailable original writable.
    let current_revision = revision(source).ok()?;
    let valid = current_revision == stamp
        && fs::metadata(&local)
            .map(|m| m.len() == size.max(0) as u64)
            .unwrap_or(false)
        && hash_file(&local).ok().as_deref() == Some(&hash);
    if !valid {
        if owned_remove(&local).is_ok() {
            let _ = with_db(|db| {
                db.execute("DELETE FROM raw_cache WHERE asset_id=?1", [id])
                    .map(|_| ())
            });
        }
        return None;
    }
    let _ = with_db(|db| {
        db.execute(
            "UPDATE raw_cache SET used=?2,path=?3 WHERE asset_id=?1",
            params![id, now(), source.to_string_lossy()],
        )
        .map(|_| ())
    });
    Some(local)
}
pub fn remember_bytes(source: &Path, bytes: &[u8], expected_revision: Option<&str>) {
    if !crate::formats::is_raw_file(source) || !eligible(source).unwrap_or(false) {
        return;
    }
    let result = (|| -> Result<()> {
        let Ok(_guard) = COPY_LOCK.try_lock() else {
            return Ok(());
        };
        let stamp = revision(source).map_err(|e| e.to_string())?;
        if Some(stamp.as_str()) != expected_revision {
            return Ok(());
        }
        let logical = source.to_string_lossy();
        let id = with_db(|db| {
            db.query_row(
                "SELECT id FROM assets WHERE path_key=?1",
                [normalized(&logical)],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })?;
        let Some(id) = id else {
            return Ok(());
        };
        let present = with_db(|db| {
            db.query_row(
                "SELECT count(*) FROM raw_cache WHERE asset_id=?1 AND revision=?2",
                params![id, stamp],
                |r| r.get::<_, i64>(0),
            )
        })?;
        if present > 0 {
            return Ok(());
        }
        ensure_capacity(bytes.len() as u64, true)?;
        let temporary = folder()?.join(format!("{id}.partial"));
        let destination = folder()?.join(format!("{id}.raw"));
        let mut output = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        output.write_all(bytes).map_err(|e| e.to_string())?;
        allocated(bytes.len() as u64);
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        let hash = blake3::hash(bytes).to_hex().to_string();
        if hash_file(&temporary)? != hash || revision(source).ok().as_deref() != Some(&stamp) {
            owned_remove(&temporary)?;
            return Err("Cache verification failed".into());
        }
        if destination.exists() {
            owned_remove(&destination)?;
        }
        fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
        with_db(|db| {
            db.execute("INSERT OR REPLACE INTO raw_cache(asset_id,path,revision,local_path,hash,size,used,pinned) VALUES(?1,?2,?3,?4,?5,?6,?7,EXISTS(SELECT 1 FROM pins WHERE asset_id=?1))",params![id,logical,stamp,destination.to_string_lossy(),hash,bytes.len() as i64,now()]).map(|_|())
        })?;
        Ok(())
    })();
    if let Err(error) = result {
        log::debug!("Recent-source caching skipped: {error}");
    }
}
#[tauri::command]
pub async fn library_cache(paths: Vec<String>, action: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        if action == "cache" || action == "pin" {
            let paths: Vec<_> = paths.into_iter().filter(|path| eligible(&ImageKey::parse(path).source).unwrap_or(false)).collect();
            if paths.is_empty() { return Err("No photos need a faster working copy. Configure source storage and a separate SSD cache in Library utilities.".into()); }
            super::jobs::enqueue_paths(
                if action == "pin" {
                    "cache-pin"
}
 else {
                    "cache"
}
,
                paths,
            )?;

}
 else if action == "unpin" {
            for path in paths {
                let source = ImageKey::parse(&path).source;
                with_db(|db|db.execute("DELETE FROM pins WHERE asset_id IN(SELECT id FROM assets WHERE path_key=?1)",[normalized(&source.to_string_lossy())]).map(|_|()))?;
                with_db(|db| {
                    db.execute(
                        "UPDATE raw_cache SET pinned=0 WHERE asset_id IN(SELECT id FROM assets WHERE path_key=?1)",
                        [normalized(&source.to_string_lossy())],
                    )
                    .map(|_| ())
}
)?;

}

}
 else {
            return Err("Unknown cache action".into());

}
        changed();
        Ok(())
}
)
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn library_exact_duplicates(paths: Vec<String>) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut groups: HashMap<(u64, String), Vec<String>> = HashMap::new();
        let mut sizes: HashMap<u64, Vec<String>> = HashMap::new();
        for path in paths {
            let key = ImageKey::parse(&path);
            if key.is_primary() {
                let size = fs::metadata(&key.source).map_err(|e| e.to_string())?.len();
                sizes.entry(size).or_default().push(path);
            }
        }
        for (size, paths) in sizes {
            if paths.len() > 1 {
                for path in paths {
                    groups
                        .entry((size, hash_file(&ImageKey::parse(&path).source)?))
                        .or_default()
                        .push(path);
                }
            }
        }
        Ok(json!(
            groups
                .into_values()
                .filter(|p| p.len() > 1)
                .collect::<Vec<_>>()
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn library_focus_check(path: String) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || native_detail(path))
        .await
        .map_err(|e| e.to_string())?
}

fn native_detail(path: String) -> Result<Value> {
    let source = ImageKey::parse(&path).source;
    let stamp = revision(&source).map_err(|e| e.to_string())?;
    let prior = with_db(|db| {
        db.query_row("SELECT data FROM analysis WHERE asset_id=(SELECT id FROM assets WHERE path_key=?1) AND revision=?2 AND version=1",params![normalized(&path),stamp],|r|r.get::<_,String>(0)).optional()
    })?;
    if let Some(prior) = prior {
        return serde_json::from_str(&prior).map_err(|e| e.to_string());
    }
    let settings = load_settings(app()?)?;
    let (image, _) = crate::image_loader::decode_pristine(&source, &path, &settings, None)?;
    let gray = image.to_luma8();
    let (w, h) = gray.dimensions();
    let mut tiles = vec![];
    for ty in 0..3 {
        for tx in 0..3 {
            let mut sum = 0.;
            let mut square = 0.;
            let mut n = 0.;
            for y in ((ty * h / 3 + 1)..((ty + 1) * h / 3).min(h.saturating_sub(1))).step_by(2) {
                for x in ((tx * w / 3 + 1)..((tx + 1) * w / 3).min(w.saturating_sub(1))).step_by(2)
                {
                    let l = 4. * gray[(x, y)][0] as f64
                        - gray[(x - 1, y)][0] as f64
                        - gray[(x + 1, y)][0] as f64
                        - gray[(x, y - 1)][0] as f64
                        - gray[(x, y + 1)][0] as f64;
                    sum += l;
                    square += l * l;
                    n += 1.;
                }
            }
            tiles.push(if n > 0. {
                square / n - (sum / n).powi(2)
            } else {
                0.
            });
        }
    }
    let data = json!({
    "source":"native-resolution","version":1,"tiles":tiles,"uncertain":true,"explanation":"Detail by image region. Subject focus and intentional blur require your judgment."
    }
    );
    with_db(|db| {
        db.execute("INSERT OR REPLACE INTO analysis(asset_id,revision,version,data) SELECT id,?2,1,?3 FROM assets WHERE path_key=?1",params![normalized(&path),stamp,data.to_string()]).map(|_|())
    })?;
    Ok(data)
}
