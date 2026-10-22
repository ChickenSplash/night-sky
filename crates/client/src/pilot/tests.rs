use super::*;
use crate::{Selection, input::Bindings};

fn actions(keys: &[KeyCode]) -> Actions {
    let mut input = ButtonInput::<KeyCode>::default();
    for key in keys {
        input.press(*key);
    }
    let mut actions = Actions::default();
    actions.update(
        &Bindings::default(),
        &input,
        &ButtonInput::default(),
        false,
        true,
    );
    actions
}

fn ship() -> (Fleet, String) {
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = Fleet::default();
    let id = fleet
        .spawn(
            system,
            page,
            &planet,
            400_000.0,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    (fleet, id)
}

#[test]
fn the_lever_stays_within_the_drives() {
    assert_eq!(lever(0.0, 1.0, 1.0, true), THROTTLE_RATE);
    assert_eq!(lever(0.9, 1.0, 1.0, true), 1.0);
    assert_eq!(lever(0.1, -1.0, 1.0, true), 0.1 - THROTTLE_RATE);
    assert_eq!(lever(0.1, -1.0, 1.0, false), 0.0);
}

#[test]
fn fine_thrusters_rest_in_warp_but_turning_steers() {
    let keys = actions(&[KeyCode::KeyW, KeyCode::KeyQ, KeyCode::ArrowRight]);
    let sublight = held(&keys, false);
    assert_eq!(sublight.translation, [0.0, 0.0, 1.0]);
    assert_eq!(sublight.rotation, [0.0, -1.0, 1.0]);
    let warp = held(&keys, true);
    assert_eq!(warp.translation, [0.0; 3]);
    assert_eq!(warp.rotation, sublight.rotation);
}

#[test]
fn orders_go_only_as_held_keys_change() {
    let (fleet, id) = ship();
    let live = fleet.get(&id).unwrap();
    let (first, sent) = orders(&actions(&[KeyCode::KeyW]), live, Held::default(), 0.016);
    assert_eq!(
        first,
        [Order::Translate {
            right: 0.0,
            up: 0.0,
            forward: 1.0
        }]
    );
    let (again, sent) = orders(&actions(&[KeyCode::KeyW]), live, sent, 0.016);
    assert!(again.is_empty());
    let (released, _) = orders(&actions(&[]), live, sent, 0.016);
    assert_eq!(
        released,
        [Order::Translate {
            right: 0.0,
            up: 0.0,
            forward: 0.0
        }]
    );
}

#[test]
fn shift_moves_the_lever_and_z_and_x_set_it() {
    let (fleet, id) = ship();
    let live = fleet.get(&id).unwrap();
    let (up, _) = orders(&actions(&[KeyCode::ShiftLeft]), live, Held::default(), 0.1);
    assert_eq!(
        up,
        [Order::Throttle {
            value: THROTTLE_RATE * 0.1
        }]
    );
    let (full, _) = orders(&actions(&[KeyCode::KeyZ]), live, Held::default(), 0.1);
    assert_eq!(full, [Order::Throttle { value: 1.0 }]);
    let (stop, _) = orders(&actions(&[KeyCode::KeyT]), live, Held::default(), 0.1);
    assert_eq!(stop, [Order::Stop]);
}

fn app(fleet: Fleet, id: &str) -> App {
    let mut app = App::new();
    let mut nav = Navigation::default();
    nav.select(Some(Selection::Craft(id.to_owned())));
    let mut clock = Clock::default();
    clock.0.rate = 3_600.0;
    app.insert_resource(fleet)
        .insert_resource(nav)
        .insert_resource(clock)
        .init_resource::<Time>()
        .init_resource::<Actions>()
        .init_resource::<Pilot>()
        .add_systems(Update, fly);
    app
}

fn press(app: &mut App, keys: &[KeyCode]) {
    *app.world_mut().resource_mut::<Actions>() = actions(keys);
    app.update();
}

#[test]
fn tab_pilots_the_selected_ship_at_real_time_and_leaving_keeps_the_throttle() {
    let (fleet, id) = ship();
    let mut app = app(fleet, &id);
    press(&mut app, &[KeyCode::Tab]);
    let world = app.world();
    assert_eq!(world.resource::<Pilot>().ship.as_deref(), Some(id.as_str()));
    assert!(world.resource::<Navigation>().following);
    assert_eq!(world.resource::<Clock>().0.rate, 1.0);
    press(&mut app, &[KeyCode::KeyZ, KeyCode::KeyW]);
    let controls = app
        .world()
        .resource::<Fleet>()
        .get(&id)
        .unwrap()
        .craft
        .input
        .controls;
    assert_eq!(controls.throttle, 1.0);
    assert_eq!(controls.translation, [0.0, 0.0, -1.0]);
    press(&mut app, &[KeyCode::Escape, KeyCode::KeyW]);
    let world = app.world();
    assert!(!world.resource::<Pilot>().flying());
    assert!(!world.resource::<Actions>().just_pressed(Action::Cancel));
    let controls = world
        .resource::<Fleet>()
        .get(&id)
        .unwrap()
        .craft
        .input
        .controls;
    assert_eq!(controls.throttle, 1.0);
    assert_eq!(controls.translation, [0.0; 3]);
    assert_eq!(world.resource::<Clock>().0.rate, 1.0);
}

#[test]
fn deselecting_the_ship_stops_piloting() {
    let (fleet, id) = ship();
    let mut app = app(fleet, &id);
    press(&mut app, &[KeyCode::Tab]);
    app.world_mut().resource_mut::<Navigation>().select(None);
    press(&mut app, &[]);
    assert!(!app.world().resource::<Pilot>().flying());
}

#[test]
fn the_readout_names_the_keys_to_stop() {
    let (fleet, id) = ship();
    let lines = readout(fleet.get(&id).unwrap(), &Bindings::default());
    assert_eq!(lines[0], "Piloting Ship 1 · Tab or Esc to stop");
    assert_eq!(lines[1], "Throttle 0%");
}
