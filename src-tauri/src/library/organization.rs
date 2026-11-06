use super::*;

pub(super) const VISIBLE: &str = "a.hidden=0 AND NOT EXISTS(SELECT 1 FROM excluded_sources s WHERE a.path_key=s.path_key OR substr(a.path_key,1,length(s.path_key)+1)=s.path_key||'/')";

pub(super) fn grouped() -> Result<bool> {
    with_db(|db| {
        db.query_row(
            "SELECT value='true' FROM state WHERE key='group-copies'",
            [],
            |r| r.get(0),
        )
        .optional()
    })
    .map(|value| value.unwrap_or(false))
}
#[tauri::command]
pub fn library_copy_view(grouped: bool) -> Result<()> {
    with_db(|db| {
        db.execute(
            "INSERT OR REPLACE INTO state(key,value) VALUES('group-copies',?1)",
            [if grouped { "true" } else { "false" }],
        )
        .map(|_| ())
    })?;
    changed();
    Ok(())
}
#[tauri::command]
pub async fn library_source_visibility(path: String, excluded: bool) -> Result<()> {
    if !Path::new(&path).is_absolute() {
        return Err("Choose an absolute source folder".into());
    }
    let key = normalized(&path).trim_end_matches('/').to_string();
    // Organization, edits and memberships stay in the catalog for reversible restoration.
    library_backup().await?;
    with_db(|db| {
        if excluded {
            db.execute(
                "INSERT OR REPLACE INTO excluded_sources(path_key,path) VALUES(?1,?2)",
                params![key, path],
            )?;
        } else {
            db.execute("DELETE FROM excluded_sources WHERE path_key=?1", [key])?;
        }
        Ok(())
    })?;
    changed();
    Ok(())
}
pub(super) fn exclusions() -> Result<Value> {
    with_db(|db| {
        let mut stmt=db.prepare("SELECT s.path,(SELECT count(*) FROM assets a WHERE a.hidden=0 AND (a.path_key=s.path_key OR substr(a.path_key,1,length(s.path_key)+1)=s.path_key||'/')) FROM excluded_sources s ORDER BY s.path")?;
        stmt.query_map([], |r| {
            Ok(json!({"path":r.get::<_,String>(0)?,"count":r.get::<_,i64>(1)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map(|rows| json!(rows))
    })
}

pub(super) fn copy_key(parts: [&str; 8]) -> String {
    let [id, path, revision, capture, camera, exif, signature, render] = parts;
    if path.contains("?vc=") || capture.len() < 19 || camera.trim().is_empty() {
        return id.into();
    }
    let filename = path.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    let extension = filename.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    let size = revision.split(':').next().unwrap_or("");
    if size.is_empty() || size == "unavailable" {
        return id.into();
    }
    let metadata: HashMap<String, String> = serde_json::from_str(exif).unwrap_or_default();
    let subsecond = metadata
        .get("SubSecTimeOriginal")
        .map(String::as_str)
        .unwrap_or("");
    let serial = metadata
        .get("BodySerialNumber")
        .or_else(|| metadata.get("SerialNumber"))
        .map(String::as_str)
        .unwrap_or("");
    // Require filename as well as capture metadata; never equate nearby burst frames.
    let evidence = if signature.is_empty() {
        "metadata"
    } else {
        signature
    };
    let value = [
        if signature.is_empty() {
            filename.as_str()
        } else {
            ""
        },
        extension,
        size,
        capture,
        camera.trim(),
        subsecond,
        serial,
        evidence,
        render,
    ]
    .join("\u{1f}");
    blake3::hash(value.as_bytes()).to_hex().to_string()
}
