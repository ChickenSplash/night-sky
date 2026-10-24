//! Bounded HTTP jobs applied on the main thread, never on the listener.
use super::PER_FRAME;
use crate::devtools::command_line::{Source, Typed};
use crate::scene::{CameraPose, CameraRig};
use crate::search::Asker;
use crate::stars::stream::Starfield;
use crate::{Clock, Navigation, Universe};
use bevy::prelude::*;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) type Verdict = mpsc::SyncSender<Result<(), String>>;

pub(super) enum Job {
    /// A parsed line, run as `Typed` so it is echoed and checked like the
    /// terminal's, and refused with the reason if it would do nothing.
    Command(String, Verdict),
    Capture(mpsc::SyncSender<Value>),
    /// A checked query, sent from the main thread so it matches the craft
    /// as they are then; replies with its ID.
    Search(SearchJob, mpsc::SyncSender<Result<u64, String>>),
}

pub(super) struct SearchJob {
    pub(super) query: space_simulation::search::Query,
    pub(super) origin_pc: [f64; 3],
    pub(super) work_cap: u64,
}

#[derive(Resource)]
pub(super) struct HttpApi {
    pub(super) receiver: Mutex<mpsc::Receiver<Job>>,
    pub(super) stop: Arc<AtomicBool>,
}

impl Drop for HttpApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub(super) fn receive(
    api: Res<HttpApi>,
    mut writer: MessageWriter<Typed>,
    mut commands: Commands,
    scene: (Res<Universe>, Res<Navigation>, Res<Clock>),
    camera: (Res<CameraRig>, Res<CameraPose>),
    field: Res<Starfield>,
    search: (Res<crate::search::Searches>, Res<crate::craft::Fleet>),
) {
    let Ok(receiver) = api.receiver.lock() else {
        return;
    };
    for job in receiver.try_iter().take(PER_FRAME) {
        match job {
            Job::Command(line, reply) => {
                writer.write(Typed {
                    line,
                    source: Source::Api,
                    reply: Some(reply),
                });
            }
            Job::Capture(reply) => {
                let (universe, nav, clock) = &scene;
                let (rig, pose) = &camera;
                let result = crate::devtools::tabs::tools::save_bug_report(
                    &mut commands,
                    universe,
                    nav,
                    clock,
                    (rig, pose),
                    &field,
                );
                let report = result.strip_prefix("Saved ").map(|path| {
                    let path = std::path::Path::new(path);
                    path.canonicalize().unwrap_or_else(|_| path.to_owned())
                });
                let _ = reply.try_send(json!({
                    "status": if report.is_some() { "capture_scheduled" } else { "failed" },
                    "report": report.as_ref().map_or(result.clone(), |path| path.display().to_string()),
                    "png": report.map(|path| path.with_extension("png").display().to_string()),
                    "note": "The report is saved synchronously; the PNG is written by the renderer a frame or two later.",
                }));
            }
            Job::Search(job, reply) => {
                let (universe, _, clock) = &scene;
                let (searches, fleet) = &search;
                let live = crate::search::live_now(fleet, universe, clock);
                let _ = reply.try_send(searches.hub.submit(
                    Asker::Api,
                    job.query,
                    job.origin_pc,
                    job.work_cap,
                    &live,
                ));
            }
        }
    }
}
