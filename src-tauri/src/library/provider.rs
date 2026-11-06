use super::*;
use crate::metadata_store::{MetadataProvider, Removal, SidecarProvider};
use std::io;
pub struct CatalogProvider;
fn remote(path: &Path) -> bool {
    crate::immich::is_remote(path)
        || app()
            .ok()
            .and_then(|app| app.path().app_cache_dir().ok())
            .is_some_and(|root| {
                let prefix = normalized(&root.join("immich").to_string_lossy());
                let path = normalized(&path.to_string_lossy());
                path == prefix || path.starts_with(&(prefix + "/"))
            })
}
fn io_error(error: String) -> io::Error {
    io::Error::other(error)
}
impl MetadataProvider for CatalogProvider {
    fn read(&self, key: &ImageKey) -> io::Result<Option<String>> {
        if remote(&key.source) {
            return SidecarProvider.read(key);
        }
        let value = with_db(|db| {
            db.query_row(
                "SELECT json,deleted FROM edit_records WHERE path=?1",
                [normalized(&key.virtual_path())],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()
        })
        .map_err(io_error)?;
        if let Some((json, deleted)) = value {
            return Ok(if deleted > 0 { None } else { Some(json) });
        }
        let value = SidecarProvider.read(key)?;
        if let Some(value) = &value {
            self.write(key, value)?;
        }
        Ok(value)
    }
    fn write(&self, key: &ImageKey, json: &str) -> io::Result<()> {
        if remote(&key.source) {
            return SidecarProvider.write(key, json);
        }
        if !key.source.is_file()
            && with_db(|db| {
                db.query_row(
                    "SELECT count(*) FROM assets WHERE path_key=?1",
                    [normalized(&key.virtual_path())],
                    |r| r.get::<_, i64>(0),
                )
            })
            .map_err(io_error)?
                > 0
        {
            return Err(io::Error::other("Unavailable originals are read-only"));
        }
        with_db(|db|db.execute("INSERT INTO edit_records(path,json,modified) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET json=excluded.json,modified=max(edit_records.modified+1,excluded.modified),deleted=0",params![normalized(&key.virtual_path()),json,now()]).map(|_|())).map_err(io_error)
    }
    fn create(&self, key: &ImageKey, json: &str) -> io::Result<bool> {
        if remote(&key.source) {
            return SidecarProvider.create(key, json);
        }
        if self.read(key)?.is_some() {
            return Ok(false);
        }
        with_db(|db| {
            db.execute(
                "INSERT OR IGNORE INTO edit_records(path,json,modified) VALUES(?1,?2,?3)",
                params![normalized(&key.virtual_path()), json, now()],
            )
            .map(|n| n > 0)
        })
        .map_err(io_error)
    }
    fn remove(&self, keys: &[ImageKey], removal: Removal) -> io::Result<()> {
        let cloud: Vec<_> = keys.iter().filter(|k| remote(&k.source)).cloned().collect();
        if !cloud.is_empty() {
            SidecarProvider.remove(&cloud, removal)?;
        }
        with_db(|db| {
            let tx = db.transaction()?;
            for key in keys {
                if remote(&key.source){
continue;
}
                let deleted=if removal==Removal::Trash&&(key.is_primary()||keys.iter().any(ImageKey::is_primary)){
2
}
else{
1
}
;
                tx.execute("INSERT INTO edit_records(path,json,modified,deleted) VALUES(?1,'{}',?2,?3) ON CONFLICT(path) DO UPDATE SET deleted=excluded.deleted,modified=excluded.modified",params![normalized(&key.virtual_path()),now(),deleted])?;
                if removal == Removal::Trash {
                    tx.execute(
                        "UPDATE assets SET available=0 WHERE path_key=?1",
                        [normalized(&key.virtual_path())],
                    )?;

}

}
            tx.commit()
}
)
        .map_err(io_error)
    }
    fn copy(&self, from: &ImageKey, to: &ImageKey) -> io::Result<()> {
        if remote(&from.source) || remote(&to.source) {
            if let Some(json) = self.read(from)? {
                self.write(to, &json)?;
            }
            return Ok(());
        }
        let _ = self.read(from)?;
        with_db(|db|db.execute("INSERT INTO edit_records(path,json,modified,deleted) SELECT ?2,json,?3,deleted FROM edit_records WHERE path=?1 ON CONFLICT(path) DO UPDATE SET json=excluded.json,modified=excluded.modified,deleted=excluded.deleted",params![normalized(&from.virtual_path()),normalized(&to.virtual_path()),now()]).map(|_|())).map_err(io_error)?;
        Ok(())
    }
    fn rename(&self, from: &ImageKey, to: &ImageKey) -> io::Result<()> {
        if remote(&from.source) && remote(&to.source) {
            return SidecarProvider.rename(from, to);
        }
        if remote(&from.source) || remote(&to.source) {
            self.copy(from, to)?;
            self.remove(std::slice::from_ref(from), Removal::Permanent)?;
            return Ok(());
        }
        if normalized(&from.virtual_path()) == normalized(&to.virtual_path()) {
            return Ok(());
        }
        let _ = self.read(from)?;
        with_db(|db| {
            let tx = db.transaction()?;
            tx.execute("INSERT INTO edit_records(path,json,modified,deleted) SELECT ?2,json,?3,deleted FROM edit_records WHERE path=?1 ON CONFLICT(path) DO UPDATE SET json=excluded.json,modified=excluded.modified,deleted=excluded.deleted",params![normalized(&from.virtual_path()),normalized(&to.virtual_path()),now()])?;
            tx.execute(
                "DELETE FROM edit_records WHERE path=?1",
                [normalized(&from.virtual_path())],
            )?;
            tx.execute(
                "UPDATE assets SET path=?2,path_key=?3,available=1 WHERE path_key=?1",
                params![
                    normalized(&from.virtual_path()),
                    to.virtual_path(),
                    normalized(&to.virtual_path())
                ],
            )?;
            tx.commit()
}
)
        .map_err(io_error)
    }
    fn exists(&self, key: &ImageKey) -> bool {
        self.read(key).ok().flatten().is_some()
    }
    fn modified_ms(&self, key: &ImageKey) -> Option<u64> {
        if remote(&key.source) {
            return SidecarProvider.modified_ms(key);
        }
        with_db(|db| {
            db.query_row(
                "SELECT modified FROM edit_records WHERE path=?1",
                [normalized(&key.virtual_path())],
                |r| r.get::<_, i64>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .map(|n| n.max(0) as u64)
    }
    fn quarantine(&self, key: &ImageKey, reason: &str) {
        if remote(&key.source) {
            SidecarProvider.quarantine(key, reason);
            return;
        }
        let _ = with_db(|db| {
            db.execute(
                "UPDATE edit_records SET path=path||?2 WHERE path=?1",
                params![
                    normalized(&key.virtual_path()),
                    format!(".corrupt-{}", now())
                ],
            )?;
            db.execute("INSERT OR REPLACE INTO edit_records(path,json,modified,deleted) VALUES(?1,'{}',?2,1)",params![normalized(&key.virtual_path()),now()])?;
            Ok(())
        });
        log::error!(
            "Quarantined catalog metadata for {}: {}",
            key.virtual_path(),
            reason
        );
    }
    fn records_for(&self, source: &Path) -> io::Result<Vec<ImageKey>> {
        if remote(source) {
            return SidecarProvider.records_for(source);
        }
        let mut keys = SidecarProvider.records_for(source).unwrap_or_default();
        keys.retain(|key| {
            with_db(|db| {
                db.query_row(
                    "SELECT deleted=0 FROM edit_records WHERE path=?1",
                    [normalized(&key.virtual_path())],
                    |r| r.get::<_, bool>(0),
                )
                .optional()
            })
            .ok()
            .flatten()
            .unwrap_or(true)
        });
        let indexed = with_db(|db| {
            db.query_row(
                "SELECT count(*) FROM assets WHERE path_key=?1",
                [normalized(&source.to_string_lossy())],
                |r| r.get::<_, i64>(0),
            )
        })
        .map_err(io_error)?
            > 0;
        let primary = ImageKey::primary(source);
        if indexed && !keys.contains(&primary) {
            keys.push(primary);
        }
        for key in self.records_under(source) {
            if normalized(&key.source.to_string_lossy()) == normalized(&source.to_string_lossy()) {
                let key = key.for_source(source);
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    }
    fn records_under(&self, source: &Path) -> Vec<ImageKey> {
        if remote(source) {
            return SidecarProvider.records_under(source);
        }
        let prefix = normalized(&source.to_string_lossy());
        with_db(|db|{
let mut stmt=db.prepare("SELECT path FROM edit_records WHERE deleted=0 AND (path=?1 OR substr(path,1,length(?1)+1)=?1||'/' OR substr(path,1,length(?1)+4)=?1||'?vc=')")?;
            stmt.query_map([prefix],|r|r.get::<_,String>(0))?.map(|r|r.map(|path|ImageKey::parse(&path))).collect::<rusqlite::Result<Vec<_>>>()
}
).unwrap_or_default()
    }
    fn clear_under(&self, source: &Path) -> io::Result<usize> {
        if remote(source) {
            return SidecarProvider.clear_under(source);
        }
        let keys = self.records_under(source);
        self.remove(&keys, Removal::Permanent)?;
        Ok(keys.len())
    }
    fn record_for_file(&self, path: &Path) -> Option<ImageKey> {
        SidecarProvider.record_for_file(path)
    }
    fn is_placeholder(&self, key: &ImageKey) -> bool {
        remote(&key.source) && SidecarProvider.is_placeholder(key)
    }
    fn records_in_folder(&self, dir: &Path, recursive: bool) -> Vec<ImageKey> {
        if remote(dir) {
            return SidecarProvider.records_in_folder(dir, recursive);
        }
        let mut keys = SidecarProvider.records_in_folder(dir, recursive);
        keys.extend(self.records_under(dir));
        let prefix = normalized(&dir.to_string_lossy())
            .trim_end_matches('/')
            .to_owned()
            + "/";
        let tombstones=with_db(|db|{
let mut stmt=db.prepare("SELECT path FROM edit_records WHERE deleted>0 AND path>=?1 AND path<?1||char(65535)")?;
stmt.query_map([prefix],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<std::collections::HashSet<_>>>()
}
).unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        keys.into_iter()
            .filter(|key| {
                !tombstones.contains(&normalized(&key.virtual_path()))
                    && seen.insert(normalized(&key.virtual_path()))
                    && (recursive
                        || key.source.parent().is_some_and(|p| {
                            normalized(&p.to_string_lossy()) == normalized(&dir.to_string_lossy())
                        }))
            })
            .collect()
    }
    fn folder_renamed(&self, old: &Path, new: &Path) -> io::Result<()> {
        let prefix = normalized(&old.to_string_lossy())
            .trim_end_matches('/')
            .to_string()
            + "/";
        let paths=with_db(|db|{
let mut stmt=db.prepare("SELECT path FROM assets WHERE substr(path_key,1,length(?1))=?1 UNION SELECT path FROM edit_records WHERE substr(path,1,length(?1))=?1")?;
stmt.query_map([&prefix],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()
}
).map_err(io_error)?;
        for path in paths {
            let key = ImageKey::parse(&path);
            let source = normalized(&key.source.to_string_lossy());
            if let Some(relative) = source.strip_prefix(&prefix) {
                self.rename(&key, &key.for_source(&new.join(relative)))?;
            }
        }
        with_db(|db| {
            db.execute(
                "UPDATE assets SET root=?2 WHERE root=?1",
                params![old.to_string_lossy(), new.to_string_lossy()],
            )
            .map(|_| ())
        })
        .map_err(io_error)?;
        changed();
        Ok(())
    }
    fn folder_removed(&self, dir: &Path) -> io::Result<()> {
        let prefix = normalized(&dir.to_string_lossy())
            .trim_end_matches('/')
            .to_string()
            + "/";
        with_db(|db| {
            db.execute(
                "UPDATE assets SET available=0 WHERE substr(path_key,1,length(?1))=?1",
                [prefix],
            )
            .map(|_| ())
        })
        .map_err(io_error)?;
        changed();
        Ok(())
    }
}
