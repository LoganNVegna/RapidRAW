pub(crate) mod jobs;
mod provider;
mod query_functions;
pub(crate) mod raw_cache;
use crate::{
    app_settings::load_settings,
    image_processing::{ImageFlag, ImageMetadata},
    metadata_store::ImageKey,
};
use rusqlite::{Connection, OptionalExtension, params, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, OnceLock},
};
use tauri::{AppHandle, Emitter, Manager};
static DB: LazyLock<Mutex<Option<Connection>>> = LazyLock::new(|| Mutex::new(None));
static APP: OnceLock<AppHandle> = OnceLock::new();
static ROOT: OnceLock<PathBuf> = OnceLock::new();
pub(crate) type Result<T> = std::result::Result<T, String>;
pub(crate) fn with_db<T>(action: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
    let mut guard = DB.lock().map_err(|e| e.to_string())?;
    action(guard.as_mut().ok_or("The library is not initialized")?).map_err(|e| e.to_string())
}
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
#[tauri::command]
pub async fn library_backup() -> Result<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let directory = root()?.join("Backups");
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = directory.join(format!(
            "library-{}-{}.sqlite",
            chrono::Utc::now().format("%Y-%m-%d"),
            uuid::Uuid::new_v4()
        ));
        with_db(|db| {
            db.execute("VACUUM main INTO ?1", [path.to_string_lossy().as_ref()])
                .map(|_| ())
        })?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}
pub(crate) fn normalized(path: &str) -> String {
    let value = path.replace('\\', "/");
    if cfg!(target_os = "windows") {
        value.to_lowercase()
    } else {
        value
    }
}
pub(crate) fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit("library-changed", ());
    }
}
pub(crate) fn root() -> Result<PathBuf> {
    ROOT.get().cloned().ok_or("Library not initialized".into())
}
pub(crate) fn app() -> Result<AppHandle> {
    APP.get().cloned().ok_or("Library not initialized".into())
}
pub(crate) fn cache_root() -> Result<PathBuf> {
    let folder = crate::storage::cache_dir(&app()?)?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(folder)
}
pub fn initialize(app: &AppHandle) -> Result<()> {
    let settings = load_settings(app.clone())?;
    let folder = settings
        .library_directory
        .clone()
        .map(PathBuf::from)
        .unwrap_or(
            app.path()
                .app_data_dir()
                .map_err(|e| e.to_string())?
                .join("Library"),
        );
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let cache = crate::storage::cache_dir(app)?;
    let library_path = normalized(
        &folder
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy(),
    );
    let cache_path = normalized(
        &cache
            .canonicalize()
            .map_err(|e| e.to_string())?
            .to_string_lossy(),
    );
    if library_path == cache_path || library_path.starts_with(&(cache_path + "/")) {
        return Err("The permanent library must be outside the disposable cache folder".into());
    }
    let conn = Connection::open(folder.join("library.sqlite")).map_err(|e| e.to_string())?;
    query_functions::register(&conn).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    conn.execute_batch(include_str!("library/schema.sql"))
        .map_err(|e| e.to_string())?;
    let columns = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(assets)")
            .map_err(|e| e.to_string())?;
        stmt.query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?
    };
    if !columns.iter().any(|c| c == "metadata_revision") || !columns.iter().any(|c| c == "hidden") {
        let recovery = folder.join("Migrations");
        fs::create_dir_all(&recovery).map_err(|e| e.to_string())?;
        conn.execute(
            "VACUUM main INTO ?1",
            [recovery
                .join(format!(
                    "before-metadata-index-{}.sqlite",
                    uuid::Uuid::new_v4()
                ))
                .to_string_lossy()
                .as_ref()],
        )
        .map_err(|e| e.to_string())?;
        if !columns.iter().any(|c| c == "metadata_revision") {
            conn.execute_batch(
                "ALTER TABLE assets ADD COLUMN metadata_revision TEXT NOT NULL DEFAULT '';",
            )
            .map_err(|e| e.to_string())?;
        }
        if !columns.iter().any(|c| c == "hidden") {
            conn.execute_batch("ALTER TABLE assets ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;")
                .map_err(|e| e.to_string())?;
        }
    }
    let _ = ROOT.set(folder);
    let _ = APP.set(app.clone());
    *DB.lock().map_err(|e| e.to_string())? = Some(conn);
    raw_cache::seed_settings(&settings)?;
    let analysis = cache_root()?.join("analysis.sqlite");
    with_db(|db| {
        db.execute(
            "ATTACH DATABASE ?1 AS disposable",
            [analysis.to_string_lossy().as_ref()],
        )?;
        db.execute_batch("PRAGMA disposable.journal_mode=WAL;PRAGMA disposable.synchronous=NORMAL;CREATE TABLE IF NOT EXISTS disposable.analysis(asset_id TEXT NOT NULL,revision TEXT NOT NULL,version INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(asset_id,version));")
    })?;
    with_db(|db| {
        db.execute("UPDATE jobs SET status='paused' WHERE status='running'", [])?;
        Ok(())
    })?;
    if let Err(error) = raw_cache::clean_interrupted_copies() {
        log::warn!("Cache cleanup deferred: {error}");
    }
    let roots = with_db(|db| {
        let mut stmt = db.prepare("SELECT DISTINCT root FROM assets WHERE root<>''")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    for root in roots {
        if !Path::new(&root).is_dir() {
            with_db(|db| {
                db.execute("UPDATE assets SET available=0 WHERE root=?1", [root])
                    .map(|_| ())
            })?;
        }
    }
    if settings.catalog_metadata_storage {
        crate::metadata_store::set_provider(Arc::new(provider::CatalogProvider));
    }
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if foreground_busy() {
                continue;
            }
            if let Err(error) = raw_cache::ensure_capacity(0, false) {
                log::debug!("Cache maintenance deferred: {error}");
            }
        }
    });
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let previous = with_db(|db| {
        db.query_row("SELECT value FROM state WHERE key='last-backup'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()
    })?;
    if previous.as_deref() != Some(&today) {
        tauri::async_runtime::spawn(async move {
            if library_backup().await.is_ok() {
                let _ = with_db(|db| {
                    db.execute(
                        "INSERT OR REPLACE INTO state(key,value) VALUES('last-backup',?1)",
                        [today],
                    )
                    .map(|_| ())
                });
            }
        });
    }
    Ok(())
}
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    pub field: String,
    pub operator: String,
    pub value: String,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryQuery {
    pub view: String,
    pub collection_id: Option<String>,
    pub folder: Option<String>,
    pub recursive: bool,
    pub rules: Vec<Rule>,
    pub mode: String,
    pub search: String,
    pub offset: u32,
    pub limit: u32,
    pub sort: String,
    pub descending: bool,
    pub colors: Vec<String>,
    pub tags: Vec<String>,
    pub tag_mode: String,
}
impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            view: "all".into(),
            collection_id: None,
            folder: None,
            recursive: false,
            rules: vec![],
            mode: "all".into(),
            search: String::new(),
            offset: 0,
            limit: 250,
            sort: "capture".into(),
            descending: false,
            colors: vec![],
            tags: vec![],
            tag_mode: "any".into(),
        }
    }
}
#[derive(Serialize, Deserialize, Clone)]
pub struct Asset {
    #[serde(rename = "assetId")]
    pub id: String,
    pub path: String,
    pub modified: u64,
    pub is_edited: bool,
    pub rating: u8,
    pub flag: Option<ImageFlag>,
    pub tags: Option<Vec<String>>,
    pub exif: Option<HashMap<String, String>>,
    pub is_raw: bool,
    pub is_virtual_copy: bool,
    pub is_cloud_placeholder: bool,
    pub group_id: Option<String>,
    pub needs_ai: bool,
    pub available: bool,
    pub review: String,
    pub capture: String,
    pub camera: String,
    pub lens: String,
    #[serde(rename = "smallPreview")]
    pub small_preview: Option<String>,
    #[serde(rename = "mediumPreview")]
    pub medium_preview: Option<String>,
    pub memberships: Vec<String>,
}
const ASSET_COLUMNS: &str = "a.id,a.path,a.modified,a.edited,a.rating,a.flag,a.tags,a.exif,a.raw,a.available,a.review,a.capture,a.camera,a.lens,a.small_preview,a.medium_preview";
fn row_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    let flag: String = row.get(5)?;
    let path: String = row.get(1)?;
    Ok(Asset {
        id: row.get(0)?,
        is_virtual_copy: path.contains("?vc="),
        path,
        modified: row.get::<_, i64>(2)?.max(0) as u64,
        is_edited: row.get(3)?,
        rating: row.get(4)?,
        flag: match flag.as_str() {
            "pick" => Some(ImageFlag::Pick),
            "reject" => Some(ImageFlag::Reject),
            _ => None,
        },
        tags: serde_json::from_str(&row.get::<_, String>(6)?).ok(),
        exif: serde_json::from_str(&row.get::<_, String>(7)?).ok(),
        is_raw: row.get(8)?,
        is_cloud_placeholder: false,
        group_id: None,
        needs_ai: false,
        available: row.get(9)?,
        review: row.get(10)?,
        capture: row.get(11)?,
        camera: row.get(12)?,
        lens: row.get(13)?,
        small_preview: row.get(14)?,
        medium_preview: row.get(15)?,
        memberships: vec![],
    })
}
pub(crate) fn revision(path: &Path) -> std::io::Result<String> {
    let m = fs::metadata(path)?;
    Ok(format!(
        "{}:{}",
        m.len(),
        m.modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
pub(crate) fn index_path(path: &str, root: &str) -> Result<()> {
    let key = ImageKey::parse(path);
    let stamp = revision(&key.source).map_err(|e| e.to_string())?;
    if key.is_primary() {
        let source = normalized(path);
        with_db(|db| {
            db.execute("UPDATE edit_records SET deleted=0 WHERE deleted=2 AND (path=?1 OR substr(path,1,length(?1)+4)=?1||'?vc=')",[source]).map(|_|())
        })?;
    }
    let metadata_stamp = crate::metadata_store::modified_ms(&key)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let unchanged = with_db(|db| {
        db.query_row(
            "SELECT revision=?2 AND metadata_revision=?3 AND available=1 FROM assets WHERE path_key=?1",
            params![normalized(path), stamp, metadata_stamp],
            |r| r.get::<_, bool>(0),
        )
        .optional()
}
)?
    .unwrap_or(false);
    if unchanged {
        return Ok(());
    }
    let mut metadata = crate::metadata_store::load(&key);
    crate::exif_processing::fill_missing_exif(&mut metadata, &key.source);
    upsert(path, root, &stamp, &metadata)?;
    let proof = source_signature(&key.source)?;
    let render = blake3::hash(
        serde_json::to_string(&metadata.adjustments)
            .unwrap_or_default()
            .as_bytes(),
    )
    .to_hex()
    .to_string();
    let metadata_stamp = crate::metadata_store::modified_ms(&key)
        .map(|n| n.to_string())
        .unwrap_or_default();
    with_db(|db| {
        db.execute(
            "UPDATE assets SET source_signature=?2,small_preview=CASE WHEN render_revision<>?3 THEN NULL ELSE small_preview END,medium_preview=CASE WHEN render_revision<>?3 THEN NULL ELSE medium_preview END,render_revision=?3,metadata_revision=?4 WHERE path_key=?1",
            params![normalized(path), proof, render, metadata_stamp],
        )
        .map(|_| ())
    })
}
fn source_signature(path: &Path) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());
    let mut bytes = vec![0; 65536];
    let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
    h.update(&bytes[..n]);
    if size > 65536 {
        file.seek(SeekFrom::End(-65536))
            .map_err(|e| e.to_string())?;
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        h.update(&bytes[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}
fn upsert(path: &str, source_root: &str, stamp: &str, meta: &ImageMetadata) -> Result<()> {
    let exif = meta.exif.clone().unwrap_or_default();
    let capture = exif
        .get("DateTimeOriginal")
        .map(|s| {
            let s = s.trim_matches('"');
            if let Some(date) = s.get(..10) {
                format!("{}{}", date.replace(':', "-"), &s[10..])
            } else {
                s.to_string()
            }
        })
        .unwrap_or_default();
    let camera = format!(
        "{} {}",
        exif.get("Make").map(String::as_str).unwrap_or(""),
        exif.get("Model").map(String::as_str).unwrap_or("")
    );
    let lens = exif.get("LensModel").cloned().unwrap_or_default();
    let flag = match meta.flag {
        Some(ImageFlag::Pick) => "pick",
        Some(ImageFlag::Reject) => "reject",
        _ => "",
    };
    let tags = meta.tags.clone().unwrap_or_default();
    let label = tags
        .iter()
        .find_map(|s| s.strip_prefix("color:"))
        .unwrap_or("");
    let raw = crate::formats::is_raw_file(path);
    let edited = crate::image_processing::is_image_edited(&meta.adjustments, raw, None);
    let modified = fs::metadata(ImageKey::parse(path).source)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|t| t.as_secs())
        .unwrap_or_default();
    with_db(|db| {
        db.execute("INSERT INTO assets(id,path,path_key,root,revision,modified,edited,rating,flag,label,tags,exif,raw,capture,camera,lens,added,available) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,1) ON CONFLICT(path_key) DO UPDATE SET path=excluded.path,revision=excluded.revision,modified=excluded.modified,edited=excluded.edited,rating=excluded.rating,flag=excluded.flag,label=excluded.label,tags=excluded.tags,exif=excluded.exif,raw=excluded.raw,capture=excluded.capture,camera=excluded.camera,lens=excluded.lens,available=1,hidden=0,small_preview=CASE WHEN assets.revision<>excluded.revision THEN NULL ELSE assets.small_preview END,medium_preview=CASE WHEN assets.revision<>excluded.revision THEN NULL ELSE assets.medium_preview END",
          params![uuid::Uuid::new_v4().to_string(),path,normalized(path),source_root,stamp,modified as i64,edited,meta.rating,flag,label,serde_json::to_string(&tags).unwrap_or_default(),serde_json::to_string(&exif).unwrap_or_default(),raw,capture,camera,lens,now()])?;
        Ok(())
    })
}
pub(crate) fn metadata_removed(
    keys: &[ImageKey],
    removal: crate::metadata_store::Removal,
) -> Result<()> {
    if ROOT.get().is_none() {
        return Ok(());
    }
    let whole_source = keys.iter().any(ImageKey::is_primary);
    with_db(|db| {
        let tx = db.transaction()?;
        for key in keys {
            if !key.is_primary()
                && !(removal == crate::metadata_store::Removal::Trash && whole_source)
            {
                tx.execute(
                    "UPDATE assets SET hidden=1,available=0 WHERE path_key=?1",
                    [normalized(&key.virtual_path())],
                )?;
            } else if removal == crate::metadata_store::Removal::Trash {
                tx.execute(
                    "UPDATE assets SET available=0 WHERE path_key=?1",
                    [normalized(&key.virtual_path())],
                )?;
            }
        }
        tx.commit()
    })?;
    changed();
    Ok(())
}
pub fn metadata_changed(key: &ImageKey, metadata: &ImageMetadata) {
    if ROOT.get().is_none() {
        return;
    }
    let path = key.virtual_path();
    if let Ok(Some(source_root)) = with_db(|db| {
        db.query_row(
            "SELECT root FROM assets WHERE path_key IN(?1,?2) ORDER BY path_key=?1 DESC LIMIT 1",
            params![normalized(&path), normalized(&key.source.to_string_lossy())],
            |r| r.get::<_, String>(0),
        )
        .optional()
    }) {
        if let Ok(stamp) = revision(&key.source) {
            let _ = upsert(&path, &source_root, &stamp, metadata);
        }
        let render = blake3::hash(
            serde_json::to_string(&metadata.adjustments)
                .unwrap_or_default()
                .as_bytes(),
        )
        .to_hex()
        .to_string();
        let _ = with_db(|db| {
            db.execute("UPDATE assets SET small_preview=CASE WHEN render_revision<>?2 THEN NULL ELSE small_preview END,medium_preview=CASE WHEN render_revision<>?2 THEN NULL ELSE medium_preview END,render_revision=?2 WHERE path_key=?1",params![normalized(&path),render])?;
            Ok(())
        });
        changed();
    }
}
fn rule_sql(rule: &Rule, values: &mut Vec<SqlValue>) -> Result<String> {
    let column = match rule.field.as_str() {
        "raw" => "a.raw",
        "rating" => "a.rating",
        "flag" => "a.flag",
        "label" => "a.label",
        "tag" => "a.tags",
        "date" => "substr(a.capture,1,10)",
        "camera" => "a.camera",
        "lens" => "a.lens",
        "edited" => "a.edited",
        "available" => "a.available",
        "review" => "a.review",
        "path" => "a.path",
        _ => return Err("Unsupported collection field".into()),
    };
    let operator = match rule.operator.as_str() {
        "eq" => "=",
        "ne" => "<>",
        "gte" => ">=",
        "lte" => "<=",
        "contains" => "LIKE",
        "before" => "<",
        "after" => ">",
        _ => return Err("Unsupported collection operator".into()),
    };
    if rule.field == "tag" && matches!(rule.operator.as_str(), "eq" | "ne") {
        values.push(SqlValue::Text(rule.value.clone()));
        values.push(SqlValue::Text(format!("user:{}", rule.value)));
        return Ok(format!(
            "{}EXISTS(SELECT 1 FROM json_each(a.tags) WHERE value IN(?,?))",
            if rule.operator == "ne" { "NOT " } else { "" }
        ));
    }
    let value = if matches!(
        rule.field.as_str(),
        "rating" | "edited" | "available" | "raw"
    ) {
        SqlValue::Integer(
            rule.value
                .parse()
                .map_err(|_| "A numeric filter value is required")?,
        )
    } else {
        SqlValue::Text(if operator == "LIKE" {
            format!(
                "%{}%",
                rule.value
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        } else {
            rule.value.clone()
        })
    };
    values.push(value);
    if rule.field == "tag" && operator == "LIKE" {
        return Ok("EXISTS(SELECT 1 FROM json_each(a.tags) WHERE value NOT LIKE 'color:%' AND value LIKE ? ESCAPE '\\')".into());
    }
    if operator == "LIKE" {
        return Ok(format!("{column} LIKE ? ESCAPE '\\'"));
    }
    Ok(format!("{column} {operator} ?"))
}
#[tauri::command]
pub async fn library_query(query: LibraryQuery) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || query_page(query))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn library_state() -> Result<Value> {
    tauri::async_runtime::spawn_blocking(catalog_state)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn library_collection(action: String, collection: Value, paths: Vec<String>) -> Result<String> {
    let id = collection
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    match action.as_str() {
        "save" => {
            let name = collection["name"].as_str().unwrap_or("").trim();
            if name.is_empty() {
                return Err("A collection name is required".into());
            }
            let kind = collection["kind"].as_str().unwrap_or("collection");
            if id == "quick" && kind != "collection" {
                return Err("The quick collection must remain an ordinary collection".into());
            }
            if !matches!(kind, "collection" | "smart" | "set") {
                return Err("Invalid collection type".into());
            }
            let parent = collection["parent"].as_str();
            if parent == Some(&id) {
                return Err("A collection cannot contain itself".into());
            }
            if let Some(parent) = parent {
                let valid = with_db(|db| {
                    db.query_row(
                        "SELECT kind='set' FROM collections WHERE id=?1",
                        [parent],
                        |r| r.get::<_, bool>(0),
                    )
                    .optional()
                })?
                .unwrap_or(false);
                if !valid {
                    return Err("Choose a collection set as parent".into());
                }
                let cycle = with_db(|db| {
                    db.query_row("WITH RECURSIVE ancestors(id,parent) AS (SELECT id,parent FROM collections WHERE id=?1 UNION SELECT c.id,c.parent FROM collections c JOIN ancestors a ON c.id=a.parent) SELECT count(*) FROM ancestors WHERE id=?2",params![parent,id],|r|r.get::<_,i64>(0))
                })?;
                if cycle > 0 {
                    return Err("A collection set cannot be placed inside its descendants".into());
                }
            }
            let rules: Vec<Rule> =
                serde_json::from_value(collection["rules"].clone()).unwrap_or_default();
            for r in &rules {
                rule_sql(r, &mut vec![])?;
            }
            capture_collection_undo()?;
            with_db(|db| {
                db.execute("INSERT INTO collections(id,name,parent,kind,rules,mode) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name,parent=excluded.parent,kind=excluded.kind,rules=excluded.rules,mode=excluded.mode",params![id,name,parent,kind,serde_json::to_string(&rules).unwrap_or_default(),collection["mode"].as_str().unwrap_or("all")])?;
                Ok(())
            })?;
        }
        "delete" => {
            if id == "quick" {
                return Err("The quick collection cannot be deleted".into());
            }
            capture_collection_undo()?;
            with_db(|db| {
                db.execute("DELETE FROM collections WHERE id=?1", [&id])?;
                db.execute("DELETE FROM state WHERE key='target' AND value NOT IN(SELECT id FROM collections WHERE kind='collection')",[])?;
                Ok(())
            })?;
        }
        "target" => {
            let valid = with_db(|db| {
                db.query_row(
                    "SELECT kind='collection' FROM collections WHERE id=?1",
                    [&id],
                    |r| r.get::<_, bool>(0),
                )
                .optional()
            })?
            .unwrap_or(false);
            if !valid {
                return Err("Choose an ordinary collection as target".into());
            }
            with_db(|db| {
                db.execute(
                    "INSERT OR REPLACE INTO state(key,value) VALUES('target',?1)",
                    [&id],
                )?;
                Ok(())
            })?;
        }
        "add" | "remove" | "toggle" | "reorder" => {
            let valid = with_db(|db| {
                db.query_row(
                    "SELECT kind='collection' FROM collections WHERE id=?1",
                    [&id],
                    |r| r.get::<_, bool>(0),
                )
                .optional()
            })?
            .unwrap_or(false);
            if !valid {
                return Err("Only ordinary collections accept manual membership".into());
            }
            capture_collection_undo()?;
            with_db(|db| {
                let tx = db.transaction()?;
                if action == "reorder" {
                    let mut ordered = vec![];
                    for path in &paths {
                        if let Some(asset)=tx.query_row("SELECT id FROM assets WHERE path_key=?1 AND id IN(SELECT asset_id FROM members WHERE collection_id=?2)",params![normalized(path),id],|r|r.get::<_,String>(0)).optional()?&& !ordered.contains(&asset){
ordered.push(asset);
}
                    }
                    let remaining = {
                        let mut stmt=tx.prepare("SELECT asset_id FROM members WHERE collection_id=?1 ORDER BY position,asset_id")?;
                        stmt.query_map([&id], |r| r.get::<_, String>(0))?
                            .collect::<rusqlite::Result<Vec<_>>>()?
                    };
                    for asset in remaining {
                        if !ordered.contains(&asset) {
                            ordered.push(asset);
                        }
                    }
                    for (position, asset) in ordered.iter().enumerate() {
                        tx.execute(
                            "UPDATE members SET position=?3 WHERE collection_id=?1 AND asset_id=?2",
                            params![id, asset, position as i64],
                        )?;
                    }
                    return tx.commit();
                }
                let mut position = tx.query_row(
                    "SELECT coalesce(max(position),0)+1 FROM members WHERE collection_id=?1",
                    [&id],
                    |r| r.get::<_, i64>(0),
                )?;
                for (order, path) in paths.iter().enumerate() {
                    let asset: Option<String> = tx
                        .query_row(
                            "SELECT id FROM assets WHERE path_key=?1",
                            [normalized(path)],
                            |r| r.get(0),
                        )
                        .optional()?;
                    if let Some(asset) = asset {
                        if action=="reorder"{
tx.execute("UPDATE members SET position=?3 WHERE collection_id=?1 AND asset_id=?2",params![id,asset,order as i64])?;
}
              else if action=="remove" || (action=="toggle" && tx.query_row("SELECT count(*) FROM members WHERE collection_id=?1 AND asset_id=?2",params![id,asset],|r|r.get::<_,u32>(0))?>0){
tx.execute("DELETE FROM members WHERE collection_id=?1 AND asset_id=?2",params![id,asset])?;
}
              else {
tx.execute("INSERT OR IGNORE INTO members(collection_id,asset_id,position) VALUES(?1,?2,?3)",params![id,asset,position])?;
position+=1;
}
                    }
                }
                tx.commit()
            })?;
        }
        _ => return Err("Unknown collection action".into()),
    }
    changed();
    Ok(id)
}
#[tauri::command]
pub fn library_review(paths: Vec<String>, status: String) -> Result<()> {
    if !matches!(status.as_str(), "reviewed" | "unreviewed" | "deferred") {
        return Err("Invalid review state".into());
    }
    for path in &paths {
        if !ImageKey::parse(path).source.is_file() {
            return Err("Unavailable photos are read-only".into());
        }
    }
    with_db(|db| {
        let tx = db.transaction()?;
        for path in paths {
            tx.execute(
                "UPDATE assets SET review=?2 WHERE path_key=?1",
                params![normalized(&path), status],
            )?;
        }
        tx.commit()
    })?;
    changed();
    Ok(())
}
#[tauri::command]
pub async fn library_resume_session(mut session: Value) -> Result<Value> {
    if session["kind"] == "culling" {
        let mut paths = vec![];
        for asset in session["assets"].as_array().into_iter().flatten() {
            let path = if let Some(id) = asset["assetId"].as_str() {
                with_db(|db| {
                    db.query_row("SELECT path FROM assets WHERE id=?1", [id], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()
                })?
            } else {
                None
            }
            .or_else(|| asset["path"].as_str().map(str::to_owned));
            if let Some(path) = path
                && ImageKey::parse(&path).source.is_file()
            {
                paths.push(path);
            }
        }
        session["paths"] = json!(paths);
        return Ok(session);
    }
    let mut query: LibraryQuery = serde_json::from_value(
        session
            .get("resolvedQuery")
            .unwrap_or(&session["query"])
            .clone(),
    )
    .map_err(|e| e.to_string())?;
    let current = session["assetId"].as_str().unwrap_or("").to_owned();
    let mut offset = 0;
    let mut first = None;
    let mut chosen = None;
    let mut reached = current.is_empty();
    let mut completed = 0;
    loop {
        query.offset = offset;
        query.limit = 1000;
        let page = library_query(query.clone()).await?;
        let rows = page["assets"].as_array().ok_or("Invalid library results")?;
        for (index, asset) in rows.iter().enumerate() {
            if asset["assetId"] == current {
                reached = true;
            }
            if asset["review"] == "reviewed" {
                completed += 1;
            }
            if asset["review"] == "unreviewed" {
                let candidate = (
                    offset + index as u32,
                    asset["assetId"].clone(),
                    asset["path"].clone(),
                );
                if first.is_none() {
                    first = Some(candidate.clone());
                }
                if reached && chosen.is_none() {
                    chosen = Some(candidate);
                }
            }
        }
        offset += rows.len() as u32;
        if offset as u64 >= page["total"].as_u64().unwrap_or(0) || rows.is_empty() {
            break;
        }
    }
    if let Some((position, id, path)) = chosen.or(first) {
        let size = session["query"]["limit"]
            .as_u64()
            .unwrap_or(250)
            .clamp(1, 1000) as u32;
        session["query"]["offset"] = json!(position / size * size);
        session["assetId"] = id;
        session["path"] = path;
    } else {
        session["path"] = Value::Null;
        session["assetId"] = Value::Null;
    }
    session["completed"] = json!(completed);
    Ok(session)
}
#[tauri::command]
pub fn library_session(action: String, session: Value) -> Result<Value> {
    if action == "save" {
        let id = session["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut saved = session;
        saved["id"] = json!(id);
        with_db(|db| {
            db.execute(
                "INSERT OR REPLACE INTO sessions(id,data,modified) VALUES(?1,?2,?3)",
                params![id, saved.to_string(), now()],
            )?;
            Ok(())
        })?;
        changed();
        return Ok(saved);
    }
    with_db(|db| {
        let mut stmt = db.prepare("SELECT data FROM sessions ORDER BY modified DESC")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Value::Array(
            rows.into_iter()
                .filter_map(|s| serde_json::from_str(&s).ok())
                .collect(),
        ))
    })
}
#[tauri::command]
pub async fn library_import_albums(path: String) -> Result<u64> {
    tauri::async_runtime::spawn_blocking(move || import_albums(path))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn library_relink(old_root: String, new_root: String) -> Result<u64> {
    tauri::async_runtime::spawn_blocking(move || relink_folder(old_root, new_root))
        .await
        .map_err(|e| e.to_string())?
}
pub(crate) fn source_copied(from: &Path, to: &Path, records: &[ImageKey]) -> Result<()> {
    if ROOT.get().is_none() || !to.is_file() {
        return Ok(());
    }
    let source_root = with_db(|db| {
        db.query_row(
            "SELECT root FROM assets WHERE path_key=?1",
            [normalized(&from.to_string_lossy())],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })?;
    let Some(source_root) = source_root else {
        return Ok(());
    };
    let root = if to.starts_with(&source_root) {
        source_root
    } else {
        to.parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned()
    };
    index_path(&to.to_string_lossy(), &root)?;
    for record in records.iter().filter(|r| !r.is_primary()) {
        index_path(&record.for_source(to).virtual_path(), &root)?;
    }
    changed();
    Ok(())
}
pub(crate) fn path_moved(from: &Path, to: &Path) -> Result<()> {
    if ROOT.get().is_none() {
        return Ok(());
    }
    let old = normalized(&from.to_string_lossy());
    let rows = with_db(|db| {
        let mut stmt = db.prepare("SELECT id,path,root FROM assets WHERE (path_key>=?1 AND path_key<?1||char(65535)) OR root=?2")?;
        stmt.query_map(params![old, from.to_string_lossy()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut updates = vec![];
    for (id, path, source_root) in rows {
        let previous_root = source_root.clone();
        let key = ImageKey::parse(&path);
        let source = normalized(&key.source.to_string_lossy());
        let destination = if source == old {
            Some(to.to_path_buf())
        } else {
            source
                .strip_prefix(&(old.clone() + "/"))
                .map(|relative| to.join(relative))
        };
        let source_root = if normalized(&source_root) == old {
            to.to_string_lossy().into_owned()
        } else if let Some(relative) = normalized(&source_root).strip_prefix(&(old.clone() + "/")) {
            to.join(relative).to_string_lossy().into_owned()
        } else {
            source_root
        };
        if let Some(destination) = destination {
            updates.push((id, key.for_source(&destination).virtual_path(), source_root));
        } else if source_root != previous_root {
            updates.push((id, path, source_root));
        }
    }
    with_db(|db| {
        let tx = db.transaction()?;
        for (id, path, source_root) in updates {
            tx.execute(
                "UPDATE assets SET path=?2,path_key=?3,root=?4,available=1 WHERE id=?1",
                params![id, path, normalized(&path), source_root],
            )?;
        }
        tx.commit()
    })?;
    changed();
    Ok(())
}
pub(crate) static FOREGROUND: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
pub(crate) fn foreground_busy() -> bool {
    FOREGROUND.load(std::sync::atomic::Ordering::SeqCst) > 0
}
pub(crate) struct ForegroundGuard;
impl ForegroundGuard {
    pub fn new() -> Self {
        FOREGROUND.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}
impl Drop for ForegroundGuard {
    fn drop(&mut self) {
        FOREGROUND.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
static PREVIEW_GENERATION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[tauri::command]
pub async fn library_previews(paths: Vec<String>) -> Result<Value> {
    use std::sync::atomic::Ordering;
    let generation = PREVIEW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        FOREGROUND.fetch_add(1, Ordering::SeqCst);
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                FOREGROUND.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _guard = Guard;
        let handle = app()?;
        let settings = load_settings(handle.clone())?;
        let thumbs = crate::file_management::get_thumb_cache_dir(&handle)?;
        let state = handle.state::<crate::app_state::AppState>();
        let gpu = crate::gpu_processing::get_or_init_gpu_context(&state, &handle).ok();
        let mut results = serde_json::Map::new();
        for path in paths.into_iter().take(250) {
            if PREVIEW_GENERATION.load(Ordering::SeqCst) != generation {
                break;
            }
            if !ImageKey::parse(&path).source.exists() {
                continue;
            }
            raw_cache::ensure_capacity(1_048_576, false)?;
            if let Some((small, medium, _, _)) =
                crate::file_management::generate_single_thumbnail_and_cache(
                    &path,
                    &thumbs,
                    gpu.as_ref(),
                    None,
                    false,
                    &handle,
                    &settings,
                    None,
                )
            {
                with_db(|db| {
                    db.execute(
                        "UPDATE assets SET small_preview=?2,medium_preview=?3 WHERE path_key=?1",
                        params![normalized(&path), small, medium],
                    )
                    .map(|_| ())
                })?;
                results.insert(
                    path,
                    json!({
                    "smallPreview":small,"mediumPreview":medium
                    }
                    ),
                );
                if results.len().is_multiple_of(8) {
                    changed();
                }
            }
        }
        changed();
        Ok(Value::Object(results))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn capture_undo(paths: &[String]) -> Result<i64> {
    let mut records = vec![];
    for path in paths {
        let metadata = crate::metadata_store::load(&ImageKey::parse(path));
        let organization = with_db(|db| {
            db.query_row(
                "SELECT review FROM assets WHERE path_key=?1",
                [normalized(path)],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })?;
        let asset_id = with_db(|db| {
            db.query_row(
                "SELECT id FROM assets WHERE path_key=?1",
                [normalized(path)],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })?;
        records.push(json!({
        "path":path,"assetId":asset_id,"metadata":metadata,"review":organization
        }
        ));
    }
    with_db(|db| {
        db.execute(
            "INSERT INTO undo(data) VALUES(?1)",
            [json!(records).to_string()],
        )?;
        let id = db.last_insert_rowid();
        db.execute(
            "DELETE FROM undo WHERE id NOT IN(SELECT id FROM undo ORDER BY id DESC LIMIT 30)",
            [],
        )?;
        Ok(id)
    })
}
fn capture_collection_undo() -> Result<()> {
    let saved = with_db(|db| {
        let mut stmt = db.prepare("SELECT id,name,parent,kind,rules,mode FROM collections")?;
        let collections = stmt
            .query_map([], |r| {
                Ok(json!([
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?
                ]))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = db.prepare("SELECT collection_id,asset_id,position FROM members")?;
        let members = stmt
            .query_map([], |r| {
                Ok(json!([
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?
                ]))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({
        "collections":collections,"members":members
        }
        ))
    })?;
    with_db(|db| {
        db.execute("INSERT INTO undo(data) VALUES(?1)", [saved.to_string()])?;
        db.execute(
            "DELETE FROM undo WHERE id NOT IN(SELECT id FROM undo ORDER BY id DESC LIMIT 30)",
            [],
        )?;
        Ok(())
    })
}
#[tauri::command]
pub fn library_edit(paths: Vec<String>, field: String, value: Value) -> Result<i64> {
    for path in &paths {
        if !ImageKey::parse(path).source.is_file() {
            return Err(
                "Unavailable photos are read-only. Reconnect or relink their folder first.".into(),
            );
        }
    }
    let undo_id = capture_undo(&paths)?;
    if field == "review" {
        library_review(paths, value.as_str().unwrap_or("").into())?;
        return Ok(undo_id);
    }
    if !matches!(
        field.as_str(),
        "rating" | "flag" | "color" | "keywords" | "keep"
    ) {
        return Err("Unknown metadata field".into());
    }
    for path in &paths {
        crate::metadata_store::update(&ImageKey::parse(path), |m| match field.as_str() {
            "rating" => m.rating = value.as_u64().unwrap_or(0).min(5) as u8,
            "flag" => {
                m.flag = match value.as_str() {
                    Some("pick") => Some(ImageFlag::Pick),
                    Some("reject") => Some(ImageFlag::Reject),
                    _ => None,
                }
            }
            "keep" => m.flag = Some(ImageFlag::Pick),
            "color" => {
                let mut tags = m.tags.take().unwrap_or_default();
                tags.retain(|t| !t.starts_with("color:"));
                if let Some(color) = value.as_str().filter(|s| !s.is_empty()) {
                    tags.push(format!("color:{color}"));
                }
                m.tags = Some(tags);
            }
            "keywords" => {
                let mut tags = m.tags.take().unwrap_or_default();
                for tag in value
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if !tags.iter().any(|t| t == tag) {
                        tags.push(tag.to_string());
                    }
                }
                m.tags = Some(tags);
            }
            _ => {}
        })
        .map_err(|e| e.to_string())?;
    }
    if field == "keep" {
        library_review(paths, "reviewed".into())?;
    }
    changed();
    Ok(undo_id)
}
#[tauri::command]
pub fn library_undo() -> Result<()> {
    let record = with_db(|db| {
        db.query_row(
            "SELECT id,data FROM undo ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
    })?;
    let Some((id, data)) = record else {
        return Ok(());
    };
    let saved: Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    if saved.is_object() {
        with_db(|db| {
            let tx = db.transaction()?;
            tx.execute_batch(
                "PRAGMA defer_foreign_keys=ON;DELETE FROM members;DELETE FROM collections;",
            )?;
            for c in saved["collections"].as_array().into_iter().flatten() {
                tx.execute("INSERT INTO collections(id,name,parent,kind,rules,mode) VALUES(?1,?2,?3,?4,?5,?6)",params![c[0].as_str(),c[1].as_str(),c[2].as_str(),c[3].as_str(),c[4].as_str(),c[5].as_str()])?;
            }
            for m in saved["members"].as_array().into_iter().flatten() {
                tx.execute(
                    "INSERT INTO members(collection_id,asset_id,position) VALUES(?1,?2,?3)",
                    params![m[0].as_str(), m[1].as_str(), m[2].as_i64()],
                )?;
            }
            tx.execute("DELETE FROM undo WHERE id=?1", [id])?;
            tx.commit()
        })?;
        changed();
        return Ok(());
    }
    let mut records: Vec<Value> = serde_json::from_value(saved).map_err(|e| e.to_string())?;
    for row in &mut records {
        if let Some(id) = row["assetId"].as_str() {
            let path = with_db(|db| {
                db.query_row("SELECT path FROM assets WHERE id=?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .optional()
            })?;
            if let Some(path) = path {
                row["path"] = json!(path);
            }
        }
    }
    for row in &records {
        if let Some(path) = row["path"].as_str()
            && !ImageKey::parse(path).source.exists()
        {
            return Err("Reconnect the original before undoing this decision".into());
        }
    }
    let decision_snapshots = records
        .first()
        .map(|r| r["suggestionsBefore"].clone())
        .unwrap_or(Value::Null);
    for row in records {
        if let Some(path) = row["path"].as_str() {
            let metadata: ImageMetadata =
                serde_json::from_value(row["metadata"].clone()).map_err(|e| e.to_string())?;
            crate::metadata_store::update(&ImageKey::parse(path), |m| *m = metadata)
                .map_err(|e| e.to_string())?;
            if let Some(review) = row["review"].as_str() {
                library_review(vec![path.into()], review.into())?;
            }
        }
    }
    for snapshot in decision_snapshots.as_array().into_iter().flatten() {
        if let Some(id) = snapshot["id"].as_str() {
            if snapshot["previous"].is_null() {
                with_db(|db| {
                    db.execute("DELETE FROM suggestions WHERE id=?1", [id])
                        .map(|_| ())
                })?;
            } else {
                library_suggestion(
                    id.into(),
                    snapshot["previous"]["state"]
                        .as_str()
                        .unwrap_or("pending")
                        .into(),
                    snapshot["previous"]["data"].clone(),
                )?;
            }
        }
    }
    with_db(|db| db.execute("DELETE FROM undo WHERE id=?1", [id]).map(|_| ()))?;
    changed();
    Ok(())
}
#[tauri::command]
pub fn library_suggestion(id: String, state: String, data: Value) -> Result<()> {
    if !matches!(state.as_str(), "accepted" | "dismissed" | "pending") {
        return Err("Invalid suggestion state".into());
    }
    with_db(|db| {
        db.execute("INSERT INTO suggestions(id,data,state,modified) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET data=excluded.data,state=excluded.state,modified=excluded.modified",params![id,data.to_string(),state,now()]).map(|_|())
    })
}
#[tauri::command]
pub fn library_accept_suggestions(paths: Vec<String>, decisions: Vec<Value>) -> Result<()> {
    if paths.is_empty() {
        return Err("Select the photos to reject first".into());
    }
    let mut previous = vec![];
    for decision in &decisions {
        let id = decision["id"]
            .as_str()
            .ok_or("Suggestion identity is missing")?;
        let saved = with_db(|db| {
            db.query_row("SELECT state,data FROM suggestions WHERE id=?1",[id],|r|Ok(json!({
"state":r.get::<_,String>(0)?,"data":serde_json::from_str::<Value>(&r.get::<_,String>(1)?).unwrap_or(Value::Null)
}
))).optional()
        })?;
        previous.push(json!({
        "id":id,"previous":saved
        }
        ));
    }
    let undo_id = library_edit(paths, "flag".into(), json!("reject"))?;
    with_db(|db| {
        let data: String =
            db.query_row("SELECT data FROM undo WHERE id=?1", [undo_id], |r| r.get(0))?;
        let mut saved: Value = serde_json::from_str(&data).unwrap_or(json!([]));
        if let Some(first) = saved.as_array_mut().and_then(|a| a.first_mut()) {
            first["suggestionsBefore"] = json!(previous);
        }
        db.execute(
            "UPDATE undo SET data=?2 WHERE id=?1",
            params![undo_id, saved.to_string()],
        )
        .map(|_| ())
    })?;
    for decision in decisions {
        library_suggestion(
            decision["id"].as_str().unwrap_or("").into(),
            "accepted".into(),
            decision["data"].clone(),
        )?;
    }
    changed();
    Ok(())
}

#[tauri::command]
pub async fn library_folder_status(path: String) -> Result<Value> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut photos = 0;
        let mut videos = 0;
        let mut folders = 0;
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) => return Ok(json!({"available":false,"error":error.to_string()})),
        };
        // Only inspect names in the selected directory; never walk a drive for an empty-state message.
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                folders += 1;
            } else if kind.is_file() {
                if crate::formats::is_supported_image_file(entry.path()) {
                    photos += 1;
                } else if entry.path().extension().and_then(|s| s.to_str()).is_some_and(|extension| {
                    ["mp4", "mov", "avi", "mkv", "mts", "m2ts"].iter().any(|v| extension.eq_ignore_ascii_case(v))
                }) {
                    videos += 1;
                }
            }
        }
        let indexed = query_page(LibraryQuery { folder: Some(path), recursive: true, limit: 1, ..Default::default() })?;
        Ok(json!({"available":true,"photos":photos,"videos":videos,"folders":folders,"indexed":indexed["total"]}))
    }).await.map_err(|e| e.to_string())?
}

fn query_page(query: LibraryQuery) -> Result<Value> {
    let mut values = Vec::new();
    let mut conditions = vec!["a.hidden=0".to_string()];
    if let Some(folder) = &query.folder {
        let prefix = normalized(folder).trim_end_matches('/').to_string() + "/";
        conditions.push("(a.path_key>=? AND a.path_key<?||char(65535))".into());
        values.push(SqlValue::Text(prefix.clone()));
        values.push(SqlValue::Text(prefix.clone()));
        if !query.recursive {
            conditions.push("instr(substr(a.path_key,?),'/')=0".into());
            values.push(SqlValue::Integer(prefix.chars().count() as i64 + 1));
        }
    }
    if let Some(id) = &query.collection_id {
        let collection = with_db(|db| {
            db.query_row(
                "SELECT kind,rules,mode FROM collections WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
        })?;
        if let Some((kind, rules, mode)) = collection {
            if kind == "smart" {
                let rules: Vec<Rule> = serde_json::from_str(&rules).map_err(|e| e.to_string())?;
                if !rules.is_empty() {
                    let clauses = rules
                        .iter()
                        .map(|r| rule_sql(r, &mut values))
                        .collect::<Result<Vec<_>>>()?;
                    conditions.push(format!(
                        "({})",
                        clauses.join(if mode == "any" { " OR " } else { " AND " })
                    ));
                }
            } else {
                conditions
                    .push("a.id IN(SELECT asset_id FROM members WHERE collection_id=?)".into());
                values.push(SqlValue::Text(id.clone()));
            }
        } else {
            return Err("Collection no longer exists".into());
        }
    }
    match query.view.as_str() {
        "picks" => conditions.push("a.flag='pick'".into()),
        "rejected" => conditions.push("a.flag='reject'".into()),
        "unreviewed" => conditions.push("a.review='unreviewed'".into()),
        "recent" => {
            conditions.push("a.added>=?".into());
            values.push(SqlValue::Integer(now() - 7 * 86400000));
        }
        "missing" => conditions.push("a.available=0".into()),
        _ => {}
    }
    if !query.search.trim().is_empty() {
        conditions.push("(instr(lower(rr_basename(a.path)),lower(?))>0 OR instr(lower(a.tags),lower(?))>0 OR instr(lower(a.camera),lower(?))>0 OR instr(lower(a.lens),lower(?))>0)".into());
        for _ in 0..4 {
            values.push(SqlValue::Text(query.search.trim().to_string()));
        }
    }
    if !query.rules.is_empty() {
        let clauses = query
            .rules
            .iter()
            .map(|rule| rule_sql(rule, &mut values))
            .collect::<Result<Vec<_>>>()?;
        conditions.push(format!(
            "({})",
            clauses.join(if query.mode == "any" { " OR " } else { " AND " })
        ));
    }
    if !query.colors.is_empty() {
        let mut clauses = vec![];
        for color in &query.colors {
            clauses.push("a.label=?");
            values.push(SqlValue::Text(if color == "none" {
                String::new()
            } else {
                color.clone()
            }));
        }
        conditions.push(format!("({})", clauses.join(" OR ")));
    }
    if !query.tags.is_empty() {
        let mut clauses = vec![];
        for tag in &query.tags {
            clauses.push("rr_tag_match(a.tags,a.exif,a.rating,?)=1");
            values.push(SqlValue::Text(tag.clone()));
        }
        conditions.push(format!(
            "({})",
            clauses.join(if query.tag_mode == "all" {
                " AND "
            } else {
                " OR "
            })
        ));
    }
    let filter = conditions.join(" AND ");
    let direction = if query.descending { "DESC" } else { "ASC" };
    let column = match query.sort.as_str() {
        "rating" => "a.rating",
        "name" => "rr_basename(a.path) COLLATE NOCASE",
        "added" => "a.added",
        "modified" | "date" => "a.modified",
        "edited" => "a.edited",
        "iso" => "rr_exif_number(a.exif,'iso')",
        "aperture" => "rr_exif_number(a.exif,'aperture')",
        "shutter_speed" => "rr_exif_number(a.exif,'shutter_speed')",
        "focal_length" => "rr_exif_number(a.exif,'focal_length')",
        "manual" if query.collection_id.is_some() => {
            "(SELECT position FROM members WHERE asset_id=a.id AND collection_id=?)"
        }
        _ => "a.capture",
    };
    let sort = format!("{column} {direction},a.path COLLATE NOCASE");
    let total = with_db(|db| {
        db.query_row(
            &format!("SELECT count(*) FROM assets a WHERE {filter}"),
            rusqlite::params_from_iter(values.iter()),
            |r| r.get::<_, i64>(0),
        )
    })?;
    if query.sort == "manual" && query.collection_id.is_some() {
        values.push(SqlValue::Text(
            query.collection_id.clone().unwrap_or_default(),
        ));
    }
    values.push(SqlValue::Integer(query.limit.clamp(1, 1000) as i64));
    values.push(SqlValue::Integer(query.offset as i64));
    let mut assets = with_db(|db| {
        let mut stmt = db.prepare(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets a WHERE {filter} ORDER BY {sort} LIMIT ? OFFSET ?"
        ))?;
        let mut assets = stmt
            .query_map(rusqlite::params_from_iter(values.iter()), row_asset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut memberships = db.prepare("SELECT collection_id FROM members WHERE asset_id=?1")?;
        for asset in &mut assets {
            asset.memberships = memberships
                .query_map([&asset.id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        for asset in &mut assets {
            let small_missing = asset
                .small_preview
                .as_ref()
                .is_some_and(|p| !Path::new(p).is_file());
            let medium_missing = asset
                .medium_preview
                .as_ref()
                .is_some_and(|p| !Path::new(p).is_file());
            if small_missing || medium_missing {
                if small_missing {
                    asset.small_preview = None;
                }
                if medium_missing {
                    asset.medium_preview = None;
                }
                db.execute(
                    "UPDATE assets SET small_preview=?2,medium_preview=?3 WHERE id=?1",
                    params![asset.id, asset.small_preview, asset.medium_preview],
                )?;
            }
        }
        Ok(assets)
    })?;
    let mut compatible: Vec<crate::file_management::ImageFile> = assets
        .iter()
        .filter_map(|a| {
            serde_json::to_value(a)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
        })
        .collect();
    if let Ok(settings) = load_settings(app()?) {
        crate::file_management::assign_group_ids(&mut compatible, &settings);
        for (asset, image) in assets.iter_mut().zip(compatible) {
            asset.group_id = image.group_id;
        }
    }
    Ok(json!({
    "assets":assets,"total":total,"offset":query.offset,"limit":query.limit.clamp(1,1000)
    }
    ))
}

fn catalog_state() -> Result<Value> {
    let mut collections = with_db(|db| {
        let mut stmt=db.prepare("SELECT id,name,parent,kind,rules,mode FROM collections ORDER BY kind='set' DESC,name COLLATE NOCASE")?;
        let rows=stmt.query_map([],|r|Ok(json!({
"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"parent":r.get::<_,Option<String>>(2)?,"kind":r.get::<_,String>(3)?,"rules":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(json!([])),"mode":r.get::<_,String>(5)?
}
)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    for collection in &mut collections {
        let count = if collection["kind"] == "smart" {
            let rules: Vec<Rule> =
                serde_json::from_value(collection["rules"].clone()).unwrap_or_default();
            let mut values = vec![];
            let clauses = rules
                .iter()
                .map(|r| rule_sql(r, &mut values))
                .collect::<Result<Vec<_>>>()?;
            let filter = if clauses.is_empty() {
                "1=1".into()
            } else {
                clauses.join(if collection["mode"] == "any" {
                    " OR "
                } else {
                    " AND "
                })
            };
            with_db(|db| {
                db.query_row(
                    &format!("SELECT count(*) FROM assets a WHERE a.hidden=0 AND ({filter})"),
                    rusqlite::params_from_iter(values.iter()),
                    |r| r.get::<_, i64>(0),
                )
            })?
        } else {
            with_db(|db| {
                db.query_row("SELECT count(*) FROM members m JOIN assets a ON a.id=m.asset_id WHERE m.collection_id=?1 AND a.hidden=0",[collection["id"].as_str().unwrap_or("")],|r|r.get::<_,i64>(0))
            })?
        };
        collection["count"] = json!(count);
    }
    let counts = with_db(|db| {
        db.query_row("SELECT count(*),coalesce(sum(flag='pick'),0),coalesce(sum(flag='reject'),0),coalesce(sum(review='unreviewed'),0),coalesce(sum(available=0),0) FROM assets WHERE hidden=0",[],|r|Ok(json!({
"all":r.get::<_,i64>(0)?,"picks":r.get::<_,i64>(1)?,"rejected":r.get::<_,i64>(2)?,"unreviewed":r.get::<_,i64>(3)?,"missing":r.get::<_,i64>(4)?
}
)))
    })?;
    let jobs = jobs::list()?;
    let target = with_db(|db| {
        db.query_row("SELECT value FROM state WHERE key='target'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()
    })?
    .unwrap_or_else(|| "quick".into());
    Ok(json!({
    "collections":collections,"counts":counts,"jobs":jobs,"target":target,"directory":root()?.to_string_lossy(),"cache":raw_cache::stats()?,"storage":raw_cache::layout()?,"roots":with_db(|db|{
    let mut stmt=db.prepare("SELECT DISTINCT root FROM assets WHERE root<>'' ORDER BY root")?;
    stmt.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()
    }
    )?
    }
    ))
}

fn import_albums(path: String) -> Result<u64> {
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let albums: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let backup = root()?.join("Migrations");
    fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    fs::write(backup.join(format!("albums-{}.json", now())), &bytes).map_err(|e| e.to_string())?;
    let snapshot = backup.join(format!("library-{}.sqlite", now()));
    with_db(|db| {
        db.execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])
            .map(|_| ())
    })?;
    fn import(items: &Value, parent: Option<String>, count: &mut u64) -> Result<()> {
        for item in items.as_array().ok_or("Invalid RapidRAW album file")? {
            let kind = if item["type"] == "group" {
                "set"
            } else {
                "collection"
            };
            let legacy_id = format!(
                "legacy:{}",
                item["id"]
                    .as_str()
                    .ok_or("An album is missing its stable ID")?
            );
            let id = library_collection(
                "save".into(),
                json!({
                "id":legacy_id,"name":item["name"],"kind":kind,"parent":parent,"rules":[]
                }
                ),
                vec![],
            )?;
            if kind == "set" {
                import(&item["children"], Some(id), count)?;
            } else {
                for path in item["images"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if Path::new(&ImageKey::parse(path).source).exists() {
                        index_path(path, "")?;
                    } else {
                        upsert(path, "", "unavailable", &ImageMetadata::default())?;
                        with_db(|db| {
                            db.execute(
                                "UPDATE assets SET available=0 WHERE path_key=?1",
                                [normalized(path)],
                            )
                            .map(|_| ())
                        })?;
                    }
                }
                library_collection(
                    "add".into(),
                    json!({
                    "id":id
                    }
                    ),
                    item["images"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                )?;
            }
            *count += 1;
        }
        Ok(())
    }
    let mut count = 0;
    import(&albums, None, &mut count)?;
    Ok(count)
}

fn relink_folder(old_root: String, new_root: String) -> Result<u64> {
    let rows = with_db(|db| {
        let mut stmt = db.prepare("SELECT path,revision,source_signature FROM assets")?;
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let prefix = normalized(&old_root).trim_end_matches('/').to_string() + "/";
    let mut replacements = vec![];
    let mut sources = std::collections::HashSet::new();
    for (path, stamp, proof) in rows {
        let key = ImageKey::parse(&path);
        let source_key = normalized(&key.source.to_string_lossy());
        if !sources.insert(source_key.clone()) {
            continue;
        }
        let Some(relative) = source_key.strip_prefix(&prefix) else {
            continue;
        };
        let destination = Path::new(&new_root).join(relative);
        if normalized(&destination.to_string_lossy()) == source_key {
            continue;
        }
        if proof.is_empty()
            || revision(&destination).ok().as_deref() != Some(&stamp)
            || source_signature(&destination).ok().as_deref() != Some(&proof)
        {
            return Err(format!(
                "File identity did not match: {}. Relinking requires recorded size, timestamp and a content fingerprint.",
                destination.display()
            ));
        }
        let collision = with_db(|db| {
            db.query_row(
                "SELECT count(*) FROM assets WHERE path_key=?1",
                [normalized(&destination.to_string_lossy())],
                |r| r.get::<_, i64>(0),
            )
        })?;
        if collision > 0 {
            return Err(format!(
                "The destination is already indexed: {}",
                destination.display()
            ));
        }
        replacements.push((key.source, destination));
    }
    for (from, to) in &replacements {
        crate::metadata_store::move_all(from, to).map_err(|e| e.to_string())?;
    }
    path_moved(Path::new(&old_root), Path::new(&new_root))?;
    changed();
    Ok(replacements.len() as u64)
}
