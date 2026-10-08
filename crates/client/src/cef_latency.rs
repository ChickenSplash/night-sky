//! Opt-in source-frame to CEF-pixel latency probe. No changes to normal launches.
use bevy::prelude::*;
use bevy_cef_core::prelude::{RenderPaintElementType, RenderTextureMessage};
use serde::Serialize;
use std::{collections::VecDeque, time::Instant};

#[derive(Resource)]
pub struct Probe {
    sequence: u32,
    pending: VecDeque<(u32, Instant)>,
    started: Option<Instant>,
    samples: Vec<Sample>,
    next_report: Instant,
    deadline: Instant,
    motion_started: Instant,
    base_rotation: Option<Quat>,
}

#[derive(Serialize)]
struct Sample {
    seconds: f64,
    latest_source_sequence: u32,
    painted_source_sequence: u32,
    received_wall_ms: u128,
    scroll_sequence: Option<u32>,
    source_frames_behind: u32,
    source_to_received_pixels_ms: f64,
}

impl Probe {
    pub fn stamp(&mut self) -> u32 {
        self.sequence = self.sequence.wrapping_add(1);
        self.pending.push_back((self.sequence, Instant::now()));
        while self.pending.len() > 600 {
            self.pending.pop_front();
        }
        self.sequence
    }
}

pub fn install(app: &mut App) {
    if std::env::var("SPACE_UI_LATENCY_PROBE").as_deref() != Ok("1") {
        return;
    }
    app.insert_resource(Probe {
        sequence: 0,
        pending: VecDeque::new(),
        started: None,
        samples: Vec::new(),
        next_report: Instant::now(),
        deadline: Instant::now() + std::time::Duration::from_secs(25),
        motion_started: Instant::now(),
        base_rotation: None,
    })
    // Keep the secondary-monitor diagnostic comparable to focused presentation.
    .insert_resource(bevy::winit::WinitSettings::continuous())
    .add_systems(
        Update,
        motion
            .after(crate::scene::controls)
            .before(crate::scene::place_camera),
    )
    .add_systems(PostUpdate, receive)
    .add_systems(Last, stop);
    if let Some(hz) = std::env::var("SPACE_UI_PROBE_HZ")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|hz| (1..=120).contains(hz))
    {
        app.insert_resource(bevy_cef::prelude::BeginFrameInterval(
            std::time::Duration::from_secs_f64(1.0 / f64::from(hz)),
        ));
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/logs");
    std::fs::write(
        root.join("ui-latency-probe.pid"),
        std::process::id().to_string(),
    )
    .expect("Write UI latency probe PID");
}

fn motion(
    mut probe: ResMut<Probe>,
    mut nav: ResMut<crate::Navigation>,
    rig: Res<crate::scene::CameraRig>,
) {
    if nav.visit.is_none() || rig.flying() {
        return;
    }
    let base = *probe.base_rotation.get_or_insert(nav.rotation);
    nav.rotation = base
        * Quat::from_rotation_y(0.12 * (probe.motion_started.elapsed().as_secs_f32() * 2.0).sin());
}

fn stop(probe: Res<Probe>, mut exit: MessageWriter<bevy::app::AppExit>) {
    if Instant::now() >= probe.deadline {
        exit.write(bevy::app::AppExit::Success);
    }
}

fn decode(buffer: &[u8]) -> Option<u32> {
    // Opaque device-pixel tags drawn by annotations.js, in CEF's BGRA order.
    if buffer.get(..8)? != [211, 121, 53, 255, 73, 17, 91, 255] {
        return None;
    }
    Some(u32::from_le_bytes([
        *buffer.get(10)?,
        *buffer.get(14)?,
        *buffer.get(18)?,
        *buffer.get(22)?,
    ]))
}

fn receive(mut events: MessageReader<RenderTextureMessage>, mut probe: ResMut<Probe>) {
    for event in events.read() {
        if event.ty != RenderPaintElementType::View {
            continue;
        }
        let Some(sequence) = decode(&event.buffer) else {
            continue;
        };
        let Some((_, published)) = probe.pending.iter().find(|(s, _)| *s == sequence).copied()
        else {
            continue;
        };
        let now = Instant::now();
        let start = *probe.started.get_or_insert(now);
        let behind = probe.sequence.wrapping_sub(sequence);
        let latest_source_sequence = probe.sequence;
        let scroll_sequence = event.buffer.get(32..56).and_then(decode_scroll);
        probe.samples.push(Sample {
            seconds: now.duration_since(start).as_secs_f64(),
            latest_source_sequence,
            painted_source_sequence: sequence,
            received_wall_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            scroll_sequence,
            source_frames_behind: behind,
            source_to_received_pixels_ms: now.duration_since(published).as_secs_f64() * 1000.0,
        });
        if now >= probe.next_report {
            probe.next_report = now + std::time::Duration::from_secs(1);
            if let Ok(json) = serde_json::to_vec_pretty(&probe.samples) {
                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/logs");
                if let Err(error) = std::fs::write(root.join("ui-latency-probe.json"), json) {
                    warn!("Could not write UI latency probe: {error}");
                }
            }
        }
    }
}

fn decode_scroll(buffer: &[u8]) -> Option<u32> {
    if buffer.get(..8)? != [51, 203, 169, 255, 121, 43, 81, 255] {
        return None;
    }
    Some(u32::from_le_bytes([
        *buffer.get(10)?,
        *buffer.get(14)?,
        *buffer.get(18)?,
        *buffer.get(22)?,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixel_tag_decodes_only_opaque_probe_frames() {
        let mut buffer = vec![211, 121, 53, 255, 73, 17, 91, 255];
        for byte in 0x12345678u32.to_le_bytes() {
            buffer.extend([0, 0, byte, 255]);
        }
        assert_eq!(decode(&buffer), Some(0x12345678));
        buffer[0] = 0;
        assert_eq!(decode(&buffer), None);
        assert_eq!(decode(&[]), None);
    }
}
