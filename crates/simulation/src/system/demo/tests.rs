use super::*;

#[test]
fn demo_has_repeatable_multi_star_constellations() {
    let first = demo_galaxy();
    assert_eq!(first, demo_galaxy());
    assert_eq!(first.systems.len(), 6);
    assert_eq!(
        first
            .systems
            .iter()
            .map(|s| s.tasks().count())
            .sum::<usize>(),
        473
    );
    let mut ids = std::collections::HashSet::new();
    // Multiple-star pages, some with planets around more than one host.
    let pages = || {
        first
            .systems
            .iter()
            .flat_map(|s| s.pages().into_iter().map(move |p| (s, p)))
    };
    assert!(pages().any(|(s, p)| s.stars_on(p).count() > 2));
    assert!(pages().filter(|(s, p)| s.hosts(*p).len() > 1).count() >= 5);
    for system in &first.systems {
        assert!(system.pages().len() >= 4);
        let mut star_pages: Vec<_> = system.stars.iter().map(|s| s.page).collect();
        star_pages.dedup();
        assert_eq!(system.pages(), star_pages);
        for (slot, (_, task)) in system.tasks().enumerate() {
            assert_eq!(task.slot, slot as u32);
            assert!(ids.insert(&task.id));
            assert!(task.url.is_none());
        }
        for assigned in &system.page_stars {
            let cell_star = crate::galaxy::cells::cell_star(&assigned.id).unwrap();
            let primary = system.star(assigned.page).unwrap();
            assert_eq!(
                crate::star::multiple::system_key(&primary.id),
                cell_star.1.id
            );
            assert_eq!(primary.mass_solar, cell_star.1.mass_solar);
            assert!(ids.insert(&assigned.id));
        }
    }
}
