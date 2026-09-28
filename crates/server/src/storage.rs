use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use space_shared::{Galaxy, StarSystem};
use space_simulation::cells::Sky;
use std::path::Path;

pub fn initialise(path: &Path) -> Result<()> {
    Connection::open(path)?.execute_batch(
        "CREATE TABLE IF NOT EXISTS snapshots (key TEXT PRIMARY KEY, json TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS slots (
              list_id TEXT NOT NULL, task_id TEXT NOT NULL, slot INTEGER NOT NULL,
              PRIMARY KEY (list_id, task_id), UNIQUE(list_id, slot));
         CREATE TABLE IF NOT EXISTS page_stars (
              list_id TEXT NOT NULL, page INTEGER NOT NULL, star_id TEXT NOT NULL UNIQUE,
              PRIMARY KEY (list_id, page));",
    )?;
    Ok(())
}

pub fn load(path: &Path, key: &str) -> Result<Option<Galaxy>> {
    let json: Option<String> = Connection::open(path)?
        .query_row("SELECT json FROM snapshots WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    // A cache from an older layout is dropped; slots keep the next sync in place.
    Ok(json.and_then(|json| {
        serde_json::from_str(&json)
            .inspect_err(|err| eprintln!("Ignoring outdated cache: {err}"))
            .ok()
    }))
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

/// Persist each list page on a distinct, real star near galactic home. The page
/// count is derived from all allocated slots, including tasks absent from the
/// latest snapshot: old page boundaries must never shrink on a deletion.
pub fn assign_page_stars(path: &Path, system: &mut StarSystem) -> Result<()> {
    let mut db = Connection::open(path)?;
    let tx = db.transaction()?;
    let highest_stored: Option<u32> = tx.query_row(
        "SELECT MAX(slot) FROM slots WHERE list_id = ?1",
        [&system.id],
        |row| row.get(0),
    )?;
    let highest = highest_stored
        .into_iter()
        .chain(system.tasks().map(|(_, task)| task.slot))
        .max();
    let list = system.id.clone();
    space_simulation::assign_page_stars(
        system,
        highest,
        &mut Sky::default(),
        |page| -> Result<Option<String>> {
            Ok(tx
                .query_row(
                    "SELECT star_id FROM page_stars WHERE list_id = ?1 AND page = ?2",
                    params![list, page],
                    |row| row.get(0),
                )
                .optional()?)
        },
        |page, star| -> Result<bool> {
            Ok(tx.execute(
                "INSERT OR IGNORE INTO page_stars(list_id, page, star_id) VALUES (?1, ?2, ?3)",
                params![list, page, star],
            )? > 0)
        },
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slots_survive_reordering_deletion_and_restart() {
        let path =
            std::env::temp_dir().join(format!("task-space-slots-{}.sqlite3", std::process::id()));
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

    #[test]
    fn page_assignments_survive_resync_and_share_generated_bodies() {
        let path = std::env::temp_dir().join(format!(
            "task-space-page-stars-{}.sqlite3",
            std::process::id()
        ));
        initialise(&path).unwrap();
        let demo = space_simulation::demo_galaxy();
        let mut a = demo.systems[0].clone();
        let mut b = demo.systems[1].clone();
        let ids: Vec<_> = (0..35).map(|n| format!("task-{n}")).collect();
        assign_slots(&path, &a.id, &ids).unwrap();
        assign_page_stars(&path, &mut a).unwrap();
        assign_page_stars(&path, &mut b).unwrap();
        assert!(a.page_stars.len() >= 3);
        assert_ne!(a.page_stars[0].id, b.page_stars[0].id);
        let assigned = a.page_stars.clone();
        let mut from_cache = a.clone();
        from_cache.page_stars.clear();
        assign_page_stars(&path, &mut from_cache).unwrap();
        assert_eq!(from_cache.page_stars, assigned);
        assign_slots(&path, &a.id, &ids[10..]).unwrap();
        assign_page_stars(&path, &mut from_cache).unwrap();
        assert_eq!(from_cache.page_stars, assigned);
        let added: Vec<_> = (35..50).map(|n| format!("task-{n}")).collect();
        assign_slots(&path, &a.id, &added).unwrap();
        assign_page_stars(&path, &mut from_cache).unwrap();
        assert!(from_cache.page_stars.starts_with(&assigned));
        let system = &mut from_cache;
        let task = space_shared::Task {
            slot: 21,
            id: "task-21".into(),
            name: "Task 21".into(),
            description: String::new(),
            status: String::new(),
            status_type: String::new(),
            status_order: 0,
            colour: [0; 3],
            assignees: vec![],
            due_date_ms: None,
            url: None,
        };
        space_simulation::place(system, vec![task]);
        let (page, index) = space_simulation::system_page_of_slot(system, 21);
        let assigned_star = &system.page_star(page).unwrap().id;
        let body =
            &space_simulation::cells::cell_planetary_system(assigned_star).unwrap()[index as usize];
        let planet = system.on_page(page).nth(index as usize).unwrap();
        assert_eq!(planet.orbit, body.orbit);
        assert_eq!(planet.task.as_ref().unwrap().id, "task-21");
        assert_eq!(
            system.star(page).unwrap(),
            &space_shared::Star {
                page,
                ..space_simulation::cells::cell_star(assigned_star).unwrap().1
            }
        );
        let before = system.clone();
        space_simulation::shape(system);
        assert_eq!(system, &before);
        std::fs::remove_file(path).unwrap();
    }
}
