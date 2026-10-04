//! Opt-in companion webview prototype. Only UI crosses stdio; rendering stays native.
use crate::actions::UiCommand;
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

static LATEST: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();

pub fn snapshot() -> BTreeMap<String, String> {
    LATEST.get_or_init(Default::default).lock().unwrap().clone()
}

pub fn publish(name: &str, json: &str) {
    LATEST
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(name.into(), json.into());
}

#[derive(Resource)]
pub struct NativePanel {
    child: Mutex<Child>,
}

impl Drop for NativePanel {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn install(app: &mut App) -> std::io::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new("python3")
        // WebKit's dmabuf path fails on this machine's NVIDIA/Wayland stack.
        // This affects only UI compositing, never Bevy's Vulkan renderer.
        .env("WEBKIT_DISABLE_DMABUF_RENDERER", "1")
        .arg(root.join("scripts/native-html-panel.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let ready = Arc::new(AtomicBool::new(false));
    let receiving = ready.clone();
    let (tx, rx) = mpsc::sync_channel(128);
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if line == "ready" {
                receiving.store(true, Ordering::Release);
                continue;
            }
            match serde_json::from_str::<UiCommand>(&line) {
                // A separate panel must never resize the simulation viewport.
                Ok(UiCommand::Panels { .. }) => {}
                Ok(command) => {
                    let _ = tx.try_send(command);
                }
                Err(error) => eprintln!("Native panel rejected command: {error}"),
            }
        }
    });
    std::thread::spawn(move || {
        let mut previous = BTreeMap::new();
        loop {
            std::thread::sleep(Duration::from_millis(100));
            if !ready.load(Ordering::Acquire) {
                continue;
            }
            let latest = snapshot();
            for (name, json) in &latest {
                if previous.get(name) == Some(json) {
                    continue;
                }
                let packet = serde_json::json!({"name": name, "detail": json});
                if writeln!(input, "{packet}")
                    .and_then(|_| input.flush())
                    .is_err()
                {
                    return;
                }
            }
            previous = latest;
        }
    });
    app.insert_resource(NativePanel {
        child: Mutex::new(child),
    });
    app.insert_resource(crate::ui_bridge::NativeCommands(Mutex::new(rx)));
    Ok(())
}
