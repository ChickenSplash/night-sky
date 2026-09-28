//! Compact, versioned messages between the browser's star worker and renderer.
//! Cell identifiers and f64 positions are transferred without rounding.

use crate::{
    cells::Cell,
    galaxy::Component,
    stream::{Event, StarCell, StarPoint},
};
use std::time::Duration;

const VERSION: u8 = 1;

pub fn encode(events: &[Event], pending: usize, elapsed: Duration, cancelled: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(VERSION);
    out.extend_from_slice(&(pending as u32).to_le_bytes());
    out.extend_from_slice(&(cancelled as u32).to_le_bytes());
    out.extend_from_slice(&elapsed.as_secs_f64().to_le_bytes());
    out.extend_from_slice(&(events.len() as u32).to_le_bytes());
    for event in events {
        match event {
            Event::Dropped(cell) => {
                out.push(0);
                out.extend_from_slice(&cell.0.to_le_bytes());
            }
            Event::Loaded(cell) => {
                out.push(1);
                out.extend_from_slice(&cell.cell.0.to_le_bytes());
                out.extend_from_slice(&(cell.stars.len() as u32).to_le_bytes());
                for star in &cell.stars {
                    for position in star.position_pc {
                        out.extend_from_slice(&position.to_le_bytes());
                    }
                    for value in [star.luminosity_solar, star.temperature_k, star.radius_solar] {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                    out.push(star.component.index() as u8);
                    out.extend_from_slice(&star.index.to_le_bytes());
                }
            }
        }
    }
    out
}

pub fn decode(bytes: &[u8]) -> Option<(Vec<Event>, usize, Duration, usize)> {
    struct Cursor<'a> {
        bytes: &'a [u8],
        at: usize,
    }
    impl Cursor<'_> {
        fn read<const N: usize>(&mut self) -> Option<[u8; N]> {
            let end = self.at.checked_add(N)?;
            let data = self.bytes.get(self.at..end)?.try_into().ok()?;
            self.at = end;
            Some(data)
        }
    }
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.read::<1>()?[0] != VERSION {
        return None;
    }
    let pending = u32::from_le_bytes(cursor.read()?) as usize;
    let cancelled = u32::from_le_bytes(cursor.read()?) as usize;
    let seconds = f64::from_le_bytes(cursor.read()?);
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let count = u32::from_le_bytes(cursor.read()?) as usize;
    let mut events = Vec::new();
    for _ in 0..count {
        let kind = cursor.read::<1>()?[0];
        let cell = Cell(u64::from_le_bytes(cursor.read()?));
        match kind {
            0 => events.push(Event::Dropped(cell)),
            1 => {
                let count = u32::from_le_bytes(cursor.read()?) as usize;
                if count > (bytes.len() - cursor.at) / 41 {
                    return None;
                }
                let mut stars = Vec::with_capacity(count);
                for _ in 0..count {
                    let position_pc = [
                        f64::from_le_bytes(cursor.read()?),
                        f64::from_le_bytes(cursor.read()?),
                        f64::from_le_bytes(cursor.read()?),
                    ];
                    let luminosity_solar = f32::from_le_bytes(cursor.read()?);
                    let temperature_k = f32::from_le_bytes(cursor.read()?);
                    let radius_solar = f32::from_le_bytes(cursor.read()?);
                    let component = *Component::ALL.get(cursor.read::<1>()?[0] as usize)?;
                    let index = u32::from_le_bytes(cursor.read()?);
                    stars.push(StarPoint {
                        position_pc,
                        luminosity_solar,
                        temperature_k,
                        radius_solar,
                        component,
                        index,
                    });
                }
                events.push(Event::Loaded(StarCell { cell, stars }));
            }
            _ => return None,
        }
    }
    (cursor.at == bytes.len()).then_some((
        events,
        pending,
        Duration::from_secs_f64(seconds),
        cancelled,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_precise_positions_and_ids() {
        let events = vec![
            Event::Loaded(StarCell {
                cell: Cell(123),
                stars: vec![StarPoint {
                    position_pc: [12345.000000001, -4.0, 99.0],
                    luminosity_solar: 1.5,
                    temperature_k: 5772.0,
                    radius_solar: 1.0,
                    component: Component::ThinDisc,
                    index: 987_654,
                }],
            }),
            Event::Dropped(Cell(42)),
        ];
        let encoded = encode(&events, 52, Duration::from_millis(21), 2);
        let (actual, pending, elapsed, cancelled) = decode(&encoded).unwrap();
        assert_eq!(actual, events);
        assert_eq!((pending, cancelled), (52, 2));
        assert_eq!(elapsed, Duration::from_millis(21));
        assert!(decode(&encoded[..encoded.len() - 1]).is_none());
    }
}
