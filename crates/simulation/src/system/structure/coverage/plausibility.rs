//! Recover cheap fallback details omitted from the saved P2 census, not new solves.
use super::*;

#[test]
#[ignore = "audit saved 399-system census; no layered solves"]
fn saved_census_fallback_audit() {
    use std::io::Write;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tmp");
    let input = std::fs::read_to_string(dir.join("p2-step7-399/composition-coverage.tsv")).unwrap();
    let mut lines = input.lines();
    let header: Vec<_> = lines.next().unwrap().split('\t').collect();
    let column = |name| header.iter().position(|&h| h == name).unwrap();
    let rows: BTreeMap<_, _> = lines
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            (fields[column("id")].to_owned(), fields)
        })
        .collect();
    let mut output = std::fs::File::create(dir.join("p2-plausibility-fallbacks.jsonl")).unwrap();
    let mut outliers = std::fs::File::create(dir.join("p2-plausibility-outliers.jsonl")).unwrap();
    let mut count = 0;
    for component in Component::ALL {
        for index in 0..57 {
            let key = format!("composition-calibration:{component:?}:{index}");
            for planet in galactic_bodies(&key, component).planets {
                let id = planet.id();
                let row = &rows[&id];
                if row[column("outcome")] == "solved" {
                    if [
                        "composition-calibration:ThinDisc:5/1",
                        "composition-calibration:Arms:38#A/1",
                        "composition-calibration:Halo:55/0",
                        "composition-calibration:ThickDisc:23/0",
                    ]
                    .contains(&id.as_str())
                    {
                        let solved = prototype::solve(
                            &planet.body.budget,
                            planet.environment.as_ref().unwrap(),
                            coupled::Controls::default(),
                        )
                        .unwrap();
                        let record = crate::system::physical::layered(&solved);
                        check_record(&record, &planet);
                        let saved: f64 = row[column("new_radius_km")].parse().unwrap();
                        assert!(
                            (record.physical.reference_radius_m / (saved * 1e3) - 1.0).abs()
                                < 1e-12
                        );
                        let mut refinement = serde_json::Value::Null;
                        if record.surface == "envelope" {
                            let fine = prototype::solve(
                                &planet.body.budget,
                                planet.environment.as_ref().unwrap(),
                                coupled::Controls {
                                    radial: dry::Controls {
                                        relative_tolerance: 2e-11,
                                        max_step: 1.0 / 256.0,
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                            match fine {
                                Ok(fine) => {
                                    let fine = crate::system::physical::layered(&fine);
                                    let radius_change = (fine.physical.reference_radius_m
                                        / record.physical.reference_radius_m
                                        - 1.0)
                                        .abs();
                                    let inertia_change = (fine.physical.inertia_factor
                                        / record.physical.inertia_factor
                                        - 1.0)
                                        .abs();
                                    eprintln!(
                                        "Outlier {id}: tighter integration radius={radius_change:.3e}, inertia={inertia_change:.3e}"
                                    );
                                    assert!(radius_change < 1e-7 && inertia_change < 1e-7);
                                    refinement = serde_json::json!({"radius_relative_change": radius_change, "inertia_relative_change": inertia_change});
                                }
                                Err(error) => {
                                    // Audit evidence, not a passed refinement check. Never raise
                                    // production budgets or accept a partial refined solution.
                                    eprintln!(
                                        "Outlier {id}: tighter integration refused: {error:?}"
                                    );
                                    refinement =
                                        serde_json::json!({"refused": format!("{error:?}")});
                                }
                            }
                        }
                        writeln!(
                            outliers,
                            "{}",
                            serde_json::json!({"id": id, "record": record, "tightened_check": refinement})
                        )
                        .unwrap();
                    }
                    continue;
                }
                let fallback = crate::system::physical::fallback(&super::super::fallback::solve(
                    &planet.body.budget,
                    planet.environment.as_ref().unwrap(),
                    coupled::Error::InvalidInput("saved census refusal, audit reconstruction"),
                ));
                check_record(&fallback, &planet);
                let saved: f64 = row[column("fallback_radius_km")].parse().unwrap();
                assert!(
                    (fallback.physical.reference_radius_m / (saved * 1e3) - 1.0).abs() < 1e-12,
                    "{id}: changed fallback"
                );
                writeln!(
                    output,
                    "{}",
                    serde_json::json!({
                        "id": id, "original_refusal": row[column("detail")], "record": fallback,
                    })
                )
                .unwrap();
                count += 1;
            }
        }
    }
    assert_eq!(count, 315);
    eprintln!("Reconstructed and checked {count} fallback records; saved radii unchanged");
}
