use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;
static RUNNING: AtomicBool = AtomicBool::new(false);
pub(super) fn list() -> Result<Value> {
    with_db(|db| {
        let mut stmt=db.prepare("SELECT id,kind,status,completed,total,failed,current,error FROM jobs ORDER BY created DESC LIMIT 12")?;
        let rows=stmt.query_map([],|r|Ok(json!({
"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"completed":r.get::<_,i64>(3)?,"total":r.get::<_,i64>(4)?,"failed":r.get::<_,i64>(5)?,"current":r.get::<_,String>(6)?,"error":r.get::<_,String>(7)?
}
)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!(rows))
    })
}
fn status(id: &str) -> Result<String> {
    with_db(|db| db.query_row("SELECT status FROM jobs WHERE id=?1", [id], |r| r.get(0)))
}
fn pause_requested(id: &str) -> bool {
    status(id).map(|s| s != "running").unwrap_or(true)
}
pub(super) fn enqueue_paths(kind: &str, paths: Vec<String>) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let paths: std::collections::BTreeSet<_> = paths
        .into_iter()
        .map(|p| ImageKey::parse(&p).source.to_string_lossy().into_owned())
        .collect();
    with_db(|db| {
        let tx = db.transaction()?;
        tx.execute("INSERT INTO jobs(id,kind,roots,status,created,total) VALUES(?1,?2,'[]','queued',?3,?4)",params![id,kind,now(),paths.len() as i64])?;
        for path in paths {
            tx.execute(
                "INSERT INTO job_items(job_id,path) VALUES(?1,?2)",
                params![id, path],
            )?;
        }
        tx.commit()
    })?;
    kick();
    changed();
    Ok(id)
}
#[tauri::command]
pub fn library_start_job(kind: String, roots: Vec<String>) -> Result<String> {
    if !matches!(kind.as_str(), "index" | "prepare" | "previews" | "cache") {
        return Err("Unknown library job".into());
    }
    if roots.is_empty() && matches!(kind.as_str(), "index" | "prepare") {
        return Err("Choose folders to index first".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    with_db(|db| {
        db.execute(
            "INSERT INTO jobs(id,kind,roots,status,created) VALUES(?1,?2,?3,'queued',?4)",
            params![
                id,
                kind,
                serde_json::to_string(&roots).unwrap_or_default(),
                now()
            ],
        )
        .map(|_| ())
    })?;
    kick();
    changed();
    Ok(id)
}
#[tauri::command]
pub fn library_job(id: String, action: String) -> Result<()> {
    let next = match action.as_str() {
        "pause" => "paused",
        "resume" => "queued",
        "cancel" => "cancelled",
        _ => return Err("Unknown job action".into()),
    };
    with_db(|db| {
        db.execute("UPDATE jobs SET status=?2 WHERE id=?1", params![id, next])
            .map(|_| ())
    })?;
    if action == "resume" {
        kick();
    }
    changed();
    Ok(())
}
fn kick() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        loop {
            let next = with_db(|db| {
                db.query_row(
                    "SELECT id,kind,roots FROM jobs WHERE status='queued' ORDER BY created LIMIT 1",
                    [],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
            });
            let Ok(Some((id, kind, roots))) = next else {
                break;
            };
            if with_db(|db| {
                db.execute(
                    "UPDATE jobs SET status='running',error='' WHERE id=?1 AND status='queued'",
                    [&id],
                )
            })
            .unwrap_or(0)
                == 0
            {
                continue;
            }
            let roots: Vec<String> = serde_json::from_str(&roots).unwrap_or_default();
            if let Err(error) = run(&id, &kind, &roots) {
                let _ = with_db(|db| {
                    db.execute(
                        "UPDATE jobs SET status='paused',error=?2 WHERE id=?1",
                        params![id, error],
                    )
                    .map(|_| ())
                });
            } else if !pause_requested(&id) {
                let _ = with_db(|db| {
                    db.execute(
                        "UPDATE jobs SET status='complete',current='' WHERE id=?1",
                        [&id],
                    )
                    .map(|_| ())
                });
            }
            changed();
        }
        RUNNING.store(false, Ordering::SeqCst);
        if with_db(|db| {
            db.query_row("SELECT count(*) FROM jobs WHERE status='queued'", [], |r| {
                r.get::<_, u32>(0)
            })
        })
        .unwrap_or(0)
            > 0
        {
            kick();
        }
    });
}
fn enqueue_discovered(id: &str, paths: &[String]) -> Result<()> {
    with_db(|db| {
        let tx = db.transaction()?;
        {
            let mut statement =
                tx.prepare("INSERT OR IGNORE INTO job_items(job_id,path) VALUES(?1,?2)")?;
            for path in paths {
                statement.execute(params![id, path])?;
            }
        }
        tx.commit()
    })
}
fn run(id: &str, kind: &str, roots: &[String]) -> Result<()> {
    let item_count = with_db(|db| {
        db.query_row(
            "SELECT count(*) FROM job_items WHERE job_id=?1",
            [id],
            |r| r.get::<_, i64>(0),
        )
    })?;
    if item_count == 0 || matches!(kind, "index" | "prepare") {
        if matches!(kind, "index" | "prepare") {
            let excluded = [
                normalized(&root()?.to_string_lossy()),
                normalized(&cache_root()?.to_string_lossy()),
            ];
            let allowed = |path: &Path| {
                let path = normalized(&path.to_string_lossy());
                !excluded
                    .iter()
                    .any(|prefix| path == *prefix || path.starts_with(&(prefix.clone() + "/")))
            };
            let mut discovered = 0i64;
            let mut last_notice = std::time::Instant::now();
            for source_root in roots {
                if !allowed(Path::new(source_root)) {
                    return Err(
                        "Choose an original photo folder outside RapidRAW's library and cache"
                            .into(),
                    );
                }
                if !Path::new(source_root).is_dir() {
                    with_db(|db| {
                        db.execute("UPDATE assets SET available=0 WHERE root=?1", [source_root])
                            .map(|_| ())
                    })?;
                    return Err(format!("Folder unavailable: {source_root}"));
                }
                let mut pending = Vec::with_capacity(256);
                for entry in WalkDir::new(source_root)
                    .follow_links(false)
                    .into_iter()
                    .filter_entry(|entry| allowed(entry.path()))
                {
                    let entry = entry
                        .map_err(|error| format!("Folder scan could not continue: {error}"))?;
                    if pause_requested(id) {
                        return Ok(());
                    }
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let path = if crate::formats::is_supported_image_file(entry.path()) {
                        Some(entry.path().to_string_lossy().into_owned())
                    } else {
                        crate::metadata_store::record_for_file(entry.path())
                            .filter(|key| {
                                !key.is_primary()
                                    && key.source.is_file()
                                    && crate::formats::is_supported_image_file(&key.source)
                                    && crate::metadata_store::exists(key)
                            })
                            .map(|key| key.virtual_path())
                    };
                    if let Some(path) = path {
                        discovered += 1;
                        pending.push(path);
                    }
                    if pending.len() >= 256 {
                        enqueue_discovered(id, &pending)?;
                        pending.clear();
                    }
                    if last_notice.elapsed() > std::time::Duration::from_millis(250) {
                        enqueue_discovered(id, &pending)?;
                        pending.clear();
                        with_db(|db| {
                            db.execute(
                                "UPDATE jobs SET total=?2,current='Finding photos' WHERE id=?1",
                                params![id, discovered],
                            )
                            .map(|_| ())
                        })?;
                        changed();
                        last_notice = std::time::Instant::now();
                    }
                }
                enqueue_discovered(id, &pending)?;
                // The selected provider may hold virtual copies without sidecar files.
                // Enumerate them once per root, never once per original.
                let copies: Vec<_> = crate::metadata_store::records_under(Path::new(source_root))
                    .into_iter()
                    .filter(|key| {
                        !key.is_primary()
                            && key.source.is_file()
                            && crate::formats::is_supported_image_file(&key.source)
                    })
                    .map(|key| key.virtual_path())
                    .collect();
                enqueue_discovered(id, &copies)?;
            }
        } else {
            let paths = with_db(|db| {
                let mut stmt =
                    db.prepare("SELECT path FROM assets WHERE available=1 ORDER BY path")?;
                stmt.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?;
            with_db(|db| {
                let tx = db.transaction()?;
                for path in paths {
                    if roots.is_empty()
                        || roots
                            .iter()
                            .any(|root| Path::new(&ImageKey::parse(&path).source).starts_with(root))
                    {
                        tx.execute(
                            "INSERT OR IGNORE INTO job_items(job_id,path) VALUES(?1,?2)",
                            params![id, path],
                        )?;
                    }
                }
                tx.commit()
            })?;
        }
    }
    with_db(|db| {
        db.execute(
            "UPDATE jobs SET total=(SELECT count(*) FROM job_items WHERE job_id=?1) WHERE id=?1",
            [id],
        )
        .map(|_| ())
    })?;
    let app = app()?;
    let settings = load_settings(app.clone())?;
    let thumbs = crate::file_management::get_thumb_cache_dir(&app)?;
    let mut last_update = std::time::Instant::now();
    loop {
        if pause_requested(id) {
            return Ok(());
        }
        if FOREGROUND.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        }
        let path = with_db(|db| {
            db.query_row("SELECT path FROM job_items WHERE job_id=?1 AND status='pending' ORDER BY path LIMIT 1",[id],|r|r.get::<_,String>(0)).optional()
        })?;
        let Some(path) = path else { break };
        let result = match kind {
            "index" => {
                let source = ImageKey::parse(&path).source;
                let source_root = roots
                    .iter()
                    .find(|root| source.starts_with(root))
                    .map(String::as_str)
                    .unwrap_or("");
                index_path(&path, source_root)
            }
            "previews" | "prepare" => {
                if kind == "prepare" {
                    let source = ImageKey::parse(&path).source;
                    let source_root = roots
                        .iter()
                        .find(|root| source.starts_with(root))
                        .map(String::as_str)
                        .unwrap_or("");
                    index_path(&path, source_root)?;
                }
                raw_cache::ensure_capacity(1_048_576, false)?;
                let state = app.state::<crate::app_state::AppState>();
                let gpu = crate::gpu_processing::get_or_init_gpu_context(&state, &app).ok();
                crate::file_management::generate_single_thumbnail_and_cache(&path,&thumbs,gpu.as_ref(),None,false,&app,&settings,None)
            .ok_or_else(||format!("Could not render preview: {path}"))
            .and_then(|(small,medium,_,_)|{
              with_db(|db|db.execute("UPDATE assets SET small_preview=?2,medium_preview=?3 WHERE path_key=?1",params![normalized(&path),small,medium]).map(|_|()))?;
Ok(())
}
)
            }
            "cache" | "cache-pin" => {
                raw_cache::stage_cancellable(&path, kind == "cache-pin", &|| pause_requested(id))
            }
            _ => Err("Invalid job".into()),
        };
        if pause_requested(id) {
            return Ok(());
        }
        let error = result.err();
        with_db(|db| {
            let tx = db.transaction()?;
            tx.execute(
                "UPDATE job_items SET status=?3,error=?4 WHERE job_id=?1 AND path=?2",
                params![
                    id,
                    path,
                    if error.is_some() { "failed" } else { "done" },
                    error.clone().unwrap_or_default()
                ],
            )?;
            tx.execute(
                "UPDATE jobs SET completed=completed+1,failed=failed+?2,current=?3 WHERE id=?1",
                params![id, if error.is_some() { 1 } else { 0 }, path],
            )?;
            tx.commit()
        })?;
        if last_update.elapsed() > std::time::Duration::from_millis(250) {
            changed();
            last_update = std::time::Instant::now();
        }
        std::thread::yield_now();
    }
    if matches!(kind, "index" | "prepare") {
        let all = with_db(|db| {
            let mut stmt = db.prepare("SELECT path FROM assets")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for path in all {
            let source = ImageKey::parse(&path).source;
            if roots.iter().any(|r| source.starts_with(r)) && !source.exists() {
                with_db(|db| {
                    db.execute(
                        "UPDATE assets SET available=0 WHERE path_key=?1",
                        [normalized(&path)],
                    )
                    .map(|_| ())
                })?;
            }
        }
    }
    Ok(())
}
