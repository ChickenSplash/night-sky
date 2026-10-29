use super::*;

#[test]
fn adopted_facts_override_late_seeded_replies_without_changing_arrival_origin() {
    let key = "g3c0200000.2.128";
    let prepared = crate::system::generated::generated_system_with_mode(
        key,
        [0.0; 3],
        crate::system::generated::Mode::PrepareRefinement,
    )
    .unwrap();
    let (id, refinement) = prepared
        .jobs
        .iter()
        .find_map(|job| {
            let outcome = job.resolve();
            if !matches!(
                outcome,
                crate::system::structure::prototype::Outcome::Layered(_)
            ) {
                return None;
            }
            Some((
                job.id.clone(),
                crate::system::physical::prepare_refinement(
                    &job.initial_result,
                    &job.environment,
                    &outcome,
                )
                .ok()?,
            ))
        })
        .unwrap();
    let mut cache = Cache::default();
    let origin = [123.0, 456.0, 789.0];
    let candidate = Candidate {
        id: key.into(),
        position_pc: origin,
        reach_pc: 0.01,
    };
    let wait = Wait::System(candidate);
    let facts = std::collections::BTreeMap::from([(id.clone(), refinement)]);
    for _ in 0..2 {
        // A late worker reply, or a cache miss after eviction, still carries
        // deterministic initial generation. The host overlays its session facts.
        cache.accept(
            &wait,
            Reply::System {
                system: Some(prepared.system.clone()),
            },
            &Galaxy::default(),
        );
        cache.refine_planets(&facts);
        let arrival = cache.arrival(key).unwrap();
        assert_eq!(arrival.origin_pc, origin);
        assert_eq!(arrival.field.page(), 0);
        let planet = arrival
            .field
            .system()
            .planets
            .iter()
            .find(|p| p.id == id)
            .unwrap();
        assert_eq!(planet.initial_result.as_ref().unwrap().revision.sequence, 1);
        crate::system::physical::validate(planet).unwrap().unwrap();
        cache.arrivals.clear();
    }
}
