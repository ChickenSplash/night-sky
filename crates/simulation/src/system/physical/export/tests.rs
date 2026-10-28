use super::*;
use crate::system::{composition::Material, physical};
use space_shared::{RefinementStatus, ResultProvenance};

#[test]
fn every_export_has_an_honest_validated_candidate_separate_from_legacy_geometry() {
    let mut compared = 0;
    let mut different = 0;
    for seed in 0..8 {
        let bodies = crate::bodies(&format!("initial-result-export:{seed}"));
        for hosted in bodies.planets {
            let expected = hosted.body.initial_result.clone();
            let budget = hosted.body.budget;
            let candidate = &expected.record.physical;
            for material in Material::ALL {
                let saved = candidate
                    .materials
                    .iter()
                    .find(|m| m.material == material.id())
                    .unwrap();
                let tolerance = 1e-13 * saved.source_kg;
                assert_eq!(saved.source_kg, budget.source().mass_kg(material));
                assert!(
                    (saved.retained_kg - budget.retained().mass_kg(material)).abs() <= tolerance
                );
                assert!((saved.escaped_kg - budget.escaped().mass_kg(material)).abs() <= tolerance);
            }
            assert_eq!(expected.revision.sequence, 0);
            assert_eq!(expected.provenance, ResultProvenance::Fallback);
            assert_eq!(expected.refinement, RefinementStatus::Pending);
            assert_eq!(expected.record.fallback_reason, None);
            let legacy = physical::to_data(&hosted.body);
            let planet = export_planet(hosted, 7, "Test designation".into(), None);
            assert_eq!(planet.initial_result.as_ref(), Some(&expected));
            assert_eq!(planet.physical.as_ref(), Some(&legacy));
            assert_eq!(planet.radius_km, legacy.reference_radius_m / 1000.0);
            physical::validate(&planet).unwrap().unwrap();
            physical::validate_result(planet.initial_result.as_ref().unwrap()).unwrap();
            let decoded: Planet =
                serde_json::from_str(&serde_json::to_string(&planet).unwrap()).unwrap();
            assert_eq!(decoded, planet);
            different += usize::from(candidate.reference_radius_m != legacy.reference_radius_m);
            compared += 1;
        }
    }
    assert!(compared > 20);
    assert!(
        different > 0,
        "fixture must exercise distinct candidate and live projections"
    );
}

#[test]
fn older_wire_records_can_omit_the_candidate_without_inventing_a_result() {
    let mut value = serde_json::to_value(Planet::default()).unwrap();
    value.as_object_mut().unwrap().remove("initial_result");
    let decoded: Planet = serde_json::from_value(value).unwrap();
    assert!(decoded.initial_result.is_none());
}
