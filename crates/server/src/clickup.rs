use anyhow::{Context, Result};
use reqwest::{
    Client,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::Deserialize;
use space_shared::{StarSystem, TaskPlanet};
use space_simulation::{orbit_for_slot, stable_hash, star_position};
use std::{path::Path, time::Duration};

pub struct ClickUp {
    client: Client,
}

impl ClickUp {
    pub fn new(token: &str) -> Result<Self> {
        let mut auth = HeaderValue::from_str(token).context("Invalid token format")?;
        auth.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        Ok(Self {
            client: Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        // Below the lowest standard ClickUp rate limit (100 requests/minute).
        for attempt in 0..3 {
            tokio::time::sleep(Duration::from_millis(750)).await;
            let response = self
                .client
                .get(format!("https://api.clickup.com/api/v2/{path}"))
                .send()
                .await?;
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 2 {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
            return Ok(response.error_for_status()?.json().await?);
        }
        unreachable!()
    }

    pub async fn system(&self, id: &str, db: &Path) -> Result<StarSystem> {
        let list: List = self.get(&format!("list/{id}")).await?;
        let mut tasks = Vec::new();
        let mut page = 0;
        loop {
            anyhow::ensure!(page < 1000, "List pagination exceeded 1000 pages");
            // Subtasks are excluded until moon support is implemented.
            let batch: TaskPage = self
                .get(&format!(
                    "list/{id}/task?page={page}&include_closed=true&subtasks=false"
                ))
                .await?;
            let done = batch.last_page.unwrap_or(batch.tasks.len() < 100);
            tasks.extend(batch.tasks);
            if done {
                break;
            }
            page += 1;
        }
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        tasks.dedup_by(|a, b| a.id == b.id);
        let ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
        let slots = crate::storage::assign_slots(db, id, &ids)?;
        Ok(StarSystem {
            id: id.to_owned(),
            name: list.name,
            position_ly: star_position(id),
            mass_solar: 1.0,
            tasks: tasks
                .into_iter()
                .zip(slots)
                .map(|(task, slot)| TaskPlanet {
                    slot: Some(slot),
                    orbit: orbit_for_slot(&task.id, slot),
                    radius_km: 4000.0 + (stable_hash(&task.id) % 6000) as f64,
                    colour: parse_colour(&task.status.color),
                    status_order: task
                        .status
                        .orderindex
                        .as_u64()
                        .or_else(|| task.status.orderindex.as_str()?.parse().ok())
                        .unwrap_or_default() as u32,
                    status_type: task.status.kind,
                    status: task.status.status,
                    assignees: task
                        .assignees
                        .into_iter()
                        .map(|a| a.username.unwrap_or_else(|| "Unassigned name".into()))
                        .collect(),
                    due_date_ms: task.due_date.and_then(|d| d.parse().ok()),
                    url: Some(format!("https://app.clickup.com/t/{}", task.id)),
                    id: task.id,
                    name: task.name,
                    description: task.description.unwrap_or_default(),
                })
                .collect(),
        })
    }
}

#[derive(Deserialize)]
struct List {
    name: String,
}
#[derive(Deserialize)]
struct TaskPage {
    tasks: Vec<Task>,
    last_page: Option<bool>,
}
#[derive(Deserialize)]
struct Task {
    id: String,
    name: String,
    description: Option<String>,
    status: Status,
    #[serde(default)]
    assignees: Vec<Assignee>,
    due_date: Option<String>,
}
#[derive(Deserialize)]
struct Status {
    status: String,
    #[serde(default)]
    color: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    orderindex: serde_json::Value,
}
#[derive(Deserialize)]
struct Assignee {
    username: Option<String>,
}

fn parse_colour(hex: &str) -> [u8; 3] {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6
        && hex.is_ascii()
        && let Ok(value) = u32::from_str_radix(hex, 16)
    {
        return [(value >> 16) as u8, (value >> 8) as u8, value as u8];
    }
    [92, 213, 242]
}
