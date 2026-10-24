use super::*;
use crate::system::demo_galaxy;

#[test]
fn every_planet_of_a_tasked_page_exists_and_tasks_keep_their_bodies() {
    let mut system = demo_galaxy().systems.remove(0);
    for page in system.pages() {
        let star = &system.page_star(page).unwrap().id;
        let bodies = crate::galaxy::cells::cell_bodies(star).unwrap();
        let planets: Vec<_> = system.on_page(page).collect();
        assert_eq!(planets.len(), bodies.planets.len());
        for (planet, hosted) in planets.iter().zip(&bodies.planets) {
            assert_eq!(planet.orbit, hosted.body.orbit);
            assert_eq!(planet.host, hosted.host);
            assert_eq!(planet.id, format!("{}/{}", planet.host, planet.orbit_index));
        }
        assert_eq!(system.star(page).unwrap().id, bodies.stars[0].id);
    }
    // The last page is only partly tasked; the rest are bare planets.
    assert!(system.planets.iter().any(|p| p.task.is_none()));
    let before = system.clone();
    shape(&mut system);
    assert_eq!(system, before);
    // Without tasks, allocated stars and their bare planets remain.
    system.planets.clear();
    shape(&mut system);
    assert_eq!(system.pages().len(), system.page_stars.len());
    assert_eq!(
        system.planets.len() as u32,
        system
            .page_stars
            .iter()
            .map(|s| planet_count(&s.id))
            .sum::<u32>()
    );
}

#[test]
fn slots_fill_pages_in_order() {
    let mut expected = (0, 0);
    for slot in 0..500 {
        while page_capacity("list", expected.0) == expected.1 {
            expected = (expected.0 + 1, 0);
        }
        assert_eq!(page_of_slot("list", slot), expected);
        expected.1 += 1;
    }
}
