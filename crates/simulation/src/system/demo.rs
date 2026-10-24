//! Reproducible fictional content for the optional information overlay.
use super::overlay::{PageStarError, assign_page_stars, place};
use space_shared::{Galaxy, StarSystem, Task};

/// Reproducible fictional lists. Stable IDs and slots keep every demo planet in
/// the same orbit across launches, while each list has several page stars.
pub fn demo_galaxy() -> Galaxy {
    use fake::{
        Fake,
        faker::lorem::en::Sentence,
        faker::name::en::Name,
        rand::{SeedableRng, rngs::StdRng},
    };

    let lists = [
        (
            "demo-flight",
            "Flight Operations",
            Some([0.0, 0.0, 0.0]),
            [
                "Design the navigation console",
                "Map the outer systems",
                "Calibrate orbital telemetry",
                "Test the jump drive",
                "Review the mission briefing",
                "Prepare launch checklist",
            ]
            .as_slice(),
            80,
        ),
        (
            "demo-research",
            "Deep Space Research",
            Some([3.7, 0.65, -2.1]),
            [
                "Survey a new exoplanet",
                "Analyse stellar spectra",
                "Deploy the sensor array",
                "Chart the debris field",
                "Process observatory data",
                "Publish the expedition log",
            ]
            .as_slice(),
            74,
        ),
        (
            "demo-observatory",
            "Observatory Network",
            None,
            [].as_slice(),
            84,
        ),
        (
            "demo-expedition",
            "Expedition Planning",
            None,
            [].as_slice(),
            76,
        ),
        (
            "demo-engineering",
            "Station Engineering",
            None,
            [].as_slice(),
            91,
        ),
        (
            "demo-archives",
            "Celestial Archives",
            None,
            [].as_slice(),
            68,
        ),
    ];
    let verbs = [
        "Survey",
        "Calibrate",
        "Map",
        "Review",
        "Test",
        "Deploy",
        "Monitor",
        "Catalogue",
    ];
    let subjects = [
        "sensor array",
        "navigation beacon",
        "spectral atlas",
        "orbital relay",
        "telemetry feed",
        "probe trajectory",
        "stellar catalogue",
        "landing site",
        "signal archive",
        "research module",
        "observation plan",
        "flight computer",
    ];
    let mut rng = StdRng::seed_from_u64(0x5441_534b_5350_4143);
    let mut galaxy = Galaxy {
        source: "Demo · generated fictional tasks".into(),
        systems: Vec::new(),
    };
    for (id, name, position_ly, named, count) in lists {
        let tasks = (0..count)
            .map(|slot| {
                let (status, status_type, status_order, colour) = match slot % 3 {
                    0 => ("in progress", "custom", 1, [92, 213, 242]),
                    1 => ("to do", "open", 0, [169, 154, 238]),
                    _ => ("complete", "done", 2, [100, 220, 169]),
                };
                let (name, description, assignee) = match named.get(slot) {
                    Some(name) => (
                        (*name).to_owned(),
                        "A fictional task for exploring Night Sky. Select a planet, follow its orbit, or jump to another system. Connect your own lists when ready.".to_owned(),
                        "Mission control".to_owned(),
                    ),
                    None => (
                        format!(
                            "{} the {} · sector {}",
                            verbs[(slot + id.len()) % verbs.len()],
                            subjects[(slot / verbs.len() + slot) % subjects.len()],
                            slot / 12 + 1,
                        ),
                        Sentence(8..16).fake_with_rng(&mut rng),
                        Name().fake_with_rng(&mut rng),
                    ),
                };
                Task {
                    slot: slot as u32,
                    id: format!("{id}-{slot}"),
                    name,
                    description,
                    status: status.into(),
                    status_type: status_type.into(),
                    status_order,
                    colour,
                    assignees: vec![assignee],
                    due_date_ms: None,
                    url: None,
                }
            })
            .collect();
        let mut system = StarSystem {
            id: id.into(),
            name: name.into(),
            position_ly: position_ly.unwrap_or_else(|| crate::star_position(id)),
            planets: Vec::new(),
            stars: Vec::new(),
            barycentres: Vec::new(),
            page_stars: Vec::new(),
        };
        place(&mut system, tasks);
        galaxy.systems.push(system);
    }
    // Demo tasks are overlays on the same generated population as task-free stars.
    // Assign without a database: iteration order and the immutable IDs make
    // every placement repeatable, even on an offline first launch.
    let mut sky = crate::galaxy::cells::Sky::default();
    let mut occupied = std::collections::HashSet::new();
    for system in &mut galaxy.systems {
        let tasks: Vec<_> = system.tasks().map(|(_, task)| task.clone()).collect();
        let highest = tasks.iter().map(|t| t.slot).max();
        assign_page_stars(
            system,
            highest,
            &mut sky,
            |_| Ok(None),
            |_, id| Ok::<_, PageStarError>(occupied.insert(id.to_owned())),
        )
        .expect("enough visible stars near home for demo pages");
        place(system, tasks);
    }
    galaxy
}

#[cfg(test)]
mod tests;
