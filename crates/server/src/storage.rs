use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use space_shared::Galaxy;
use std::path::Path;

pub fn initialise(path: &Path) -> Result<()> {
    Connection::open(path)?.execute_batch(
        "CREATE TABLE IF NOT EXISTS snapshots (key TEXT PRIMARY KEY, json TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS slots (
             list_id TEXT NOT NULL, task_id TEXT NOT NULL, slot INTEGER NOT NULL,
             PRIMARY KEY (list_id, task_id), UNIQUE(list_id, slot));",
    )?;
    Ok(())
}

pub fn load(path: &Path, key: &str) -> Result<Option<Galaxy>> {
    let json: Option<String> = Connection::open(path)?
        .query_row("SELECT json FROM snapshots WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(json.map(|json| serde_json::from_str(&json)).transpose()?)
}

pub fn save(path: &Path, key: &str, galaxy: &Galaxy) -> Result<()> {
    Connection::open(path)?.execute(
        "INSERT INTO snapshots(key, json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET json = excluded.json",
        params![key, serde_json::to_string(galaxy)?],
    )?;
    Ok(())
}

/// Never reuse a slot: edits, removals and API ordering cannot move existing planets.
pub fn assign_slots(path: &Path, list: &str, tasks: &[String]) -> Result<Vec<u32>> {
    let mut db = Connection::open(path)?;
    let tx = db.transaction()?;
    let mut next: u32 = tx.query_row(
        "SELECT COALESCE(MAX(slot) + 1, 0) FROM slots WHERE list_id = ?1",
        [list],
        |r| r.get(0),
    )?;
    let mut result = Vec::new();
    for id in tasks {
        let slot: Option<u32> = tx
            .query_row(
                "SELECT slot FROM slots WHERE list_id = ?1 AND task_id = ?2",
                params![list, id],
                |r| r.get(0),
            )
            .optional()?;
        result.push(match slot {
            Some(slot) => slot,
            None => {
                tx.execute(
                    "INSERT INTO slots VALUES (?1, ?2, ?3)",
                    params![list, id, next],
                )?;
                let slot = next;
                next += 1;
                slot
            }
        });
    }
    tx.commit()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slots_survive_reordering_deletion_and_restart() {
        let path = std::env::temp_dir().join(format!(
            "clickup-space-slots-{}.sqlite3",
            std::process::id()
        ));
        initialise(&path).unwrap();
        assert_eq!(
            assign_slots(&path, "list", &["a".into(), "b".into()]).unwrap(),
            vec![0, 1]
        );
        assert_eq!(
            assign_slots(&path, "list", &["b".into(), "c".into()]).unwrap(),
            vec![1, 2]
        );
        assert_eq!(
            assign_slots(&path, "list", &["c".into(), "a".into()]).unwrap(),
            vec![2, 0]
        );
        assert_eq!(
            assign_slots(&path, "other", &["a".into()]).unwrap(),
            vec![0]
        );
        std::fs::remove_file(path).unwrap();
    }
}
