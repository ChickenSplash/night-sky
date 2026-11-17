# Navigation, selection and camera frames

Physical location, the open list, selection and presentation are
separate concepts. The inspector follows selection, or location when nothing is selected.
See [controls](controls.md) for user actions,
[the scene interface](ui/interface.md) for breadcrumbs and inspection, and
[the information overlay](../shared/information-overlay.md) for list pages.
Client paths below are relative to `crates/client/src/` unless prefixed with `crates/`.

## Visits and presentation

- There is one mode: the streamed galaxy. List pages are ordinary cell stars,
  drawn and picked like any other, with no markers of their own until their
  system is loaded (see Selection and picking).
- `Navigation::visit` (`navigation.rs`) is the system the camera is anchored to
  (at, flying to, or showing as a constellation): `Visit::List` (a list index,
  at `Navigation::page`) or `Visit::Generated` (task-free planets built from
  the star's ID, never added to the lists' `Universe`). `None` is open space,
  in galactic axes relative to `Navigation::galactic_origin_ly`. A visit
  moves the camera frame to its star in f64, and the same `Flight` shifts the
  displayed pose.
- Open space keeps the camera's position small, since light years from home
  are only precise to tens of kilometres across the galaxy (about 54 km per
  f64 step 26,000 ly out). Its origin is a nearby craft's anchor, else the
  system just left, moved onto the eye whenever the eye is 10⁹ m from it
  (`REANCHOR_METRES`). Commands that place the camera relative to home
  reset it with `Navigation::leave_for_home_frame`.
- `navigation/presentation.rs` owns `PresentationMode`: `Simulation` (simulated orbital
  positions), `Table` (comparison row), and `Constellation` (connected stars).
  They work on any visited system. `Navigation::presentation_mode` describes
  presentation, not tracking or free flight. Switching presets also sets framing.
- The terminal command is `set_presentation_mode simulation|table|constellation`;
  JSON uses `type: "set_presentation_mode"` and `presentation_mode`, also the
  browser state key. Camera views and physical `Orbit` elements keep their names.
- Table layout is presentation-only and keeps physical radii. Constellation
  view keeps the current page's frame, fits page stars from the current angle,
  and draws links without moving stars. Simulation continues in every mode.

## Physical arrival and departure

- `Navigation::location` (the visit while `Navigation::arrived`) is physical;
  `Navigation::list` is the open list (a `Visit::List`, kept in constellation view).
  `scene::locate` (`scene/location.rs`) sets `arrived` every frame: within
  `system_range` of the page star (0.02 ly, or 4 times the system's extent,
  `system_extent_au`, stars and planets, if wider) it arrives, and it leaves
  beyond `LEAVE_FACTOR` (2) times that.
- Constellation view is never at a star, and a flight arrives only on getting
  there. Clicking Visit and drifting in arrive the same way. Leaving in
  simulation mode, however the visit began, returns to open space and closes
  the list; there is no restoring a previous visit.
- Free flight automatically activates a loaded star's list page or generated
  system within 0.02 ly, without flying, reframing or changing movement speed.
  Frame changes preserve the camera eye despite f32 quaternion rounding.
- Bodies are drawn (`scene::body_range`) and render scale tightens only while
  `arrived`, which follows physical proximity, not zoom/range. The same
  point-to-body handoff works after clicking Visit or drifting in.
- Close-up star size exaggeration follows physical eye distance, never a
  potentially interstellar camera range that would put the eye inside its shell.
- The render scale tightens near any loaded star within 20 ly even before its
  system activates, independently of selected range. Preserve the visited
  body's physical-proximity fallback when its cell has not loaded yet.
- The body location (`Navigation::location_body`) uses simulated f64
  positions and the rule that picks a craft's orbit reference
  (`Field::select_reference`, see [craft](../simulation/craft.md)), so the
  breadcrumb, the camera's motion and a ship there agree:
  - a planet within its Hill sphere, the nearest if several;
  - else the innermost star or pair of a multiple system whose stable zone
    holds the camera. A zone comes from the pair's masses and separation, so
    a star with no planets has one too;
  - else none: the whole system is the star level, so a single star adds
    nothing.
  Each is kept until 10% past its region (`SWITCH_MARGIN`). One difference
  from craft: the camera is at a planet within at least 30 of its radii
  (`PLANET_MIN_RADII`), because a close-in planet's Hill sphere (about 7
  radii for a hot Jupiter) can be smaller than the following distance. A
  ship that close really does orbit the star, so craft have no minimum.
  `scene::locate_body` runs after camera placement, using the same
  simulation instant as the eye, including when following at high time
  rates. Render exaggeration, selection and following never affect it;
   table/constellation layouts have no physical body location.

The camera's neighbourhood field follows the complete active system record,
including physical revisions with the same system ID. It is rebuilt when those
facts change, not just on page/list changes, such as a backend snapshot.
Generated planets never change after generation. Model gas references, optical
proxies and even known solid phases never grant landing permission.

## Moving with objects

- The camera moves with `Navigation::anchor`, set each frame by
  `scene::locate_anchor` from its position alone:
  - a craft within 1,000 km (`CRAFT_NEIGHBOURHOOD_M`), the nearest first,
    kept until 10% past that;
  - else the body it is at, exactly the body location above (a planet, or
    a star or pair of a multiple system as it orbits), so the breadcrumb
    and the camera's motion never disagree;
  - else nothing: the camera stays still in its system's or galactic frame.
- `place_camera` adds the anchor's motion since the last frame to the free
  eye and target, so WASD, R/F and mouse-look move relative to it and a
  planet never races off along its orbit. Following, flights and other
  presentation modes leave the camera to them.
- Following only centres the selected object; Fly to or Follow reaches its
  neighbourhood, and stopping following keeps moving with it. Flying into
  another body's region moves with that one instead; leaving hands
  up to the frame. A handoff changes speed at once, by the anchor's speed
  relative to the frame: small at a sphere of influence's edge.
- In galactic space the frame is the anchoring craft's own galactic anchor,
  as when following, and moves with it. The eye keeps its offset from the
  craft directly, never through the frames' light-year origins, so
  re-anchoring loses no precision.
- The HUD shows "Moving with" above the velocity, named as the breadcrumb's
  last crumb, and the snapshot `anchor` (`craft` or `body`).
  Velocity stays measured in the frame, not relative to the anchor.

## Velocity readout

- `measure_velocity` gives the HUD velocity and the velocity marker, in
  galactic axes. In free flight it is the controls' exact travel this frame
  plus the exact velocity of what carries the camera (the anchor, or a
  followed planet), times the effective time rate.
- It never differences positions there: orbital positions millions of
  kilometres from their barycentre round to centimetres, and a clock
  thousands of years in steps by about 15 µs, both large errors over one
  frame. A craft carries the camera at its displayed velocity, its last
  step's motion, which includes warp.
- Flights, right-drag orbiting (whose sweep is in the speed, never the
  marker) and a followed craft are measured from displayed positions.

## Selection and picking

- `Navigation::selection` is the one clicked object: `Selection::Star` (stable
  star ID), `Selection::Planet` (stable planet ID, including remote systems),
  `Selection::Craft` (a live craft's ID, see [craft](../simulation/craft.md#in-the-client)),
  `Selection::Barycentre` (a pair of the loaded system, by member ID),
  `Selection::Galaxy` or `Selection::List` (stable list ID).
  Selecting one replaces the other; `Navigation::select` keeps following only
  from planet to planet. A second click activates it: a star is visited, a
  planet or craft followed.
- While a system is loaded (as for planet markers), each star of the page has
  a marker, single stars included, and each pair a small-cross marker at its
  moving centre of mass, the whole system's included
  (`scene::draw::member_markers`). Their labels are the breadcrumb's
  ("System 2 B", "System 2 AB"). Clicking one selects it (`SelectStar` with
  the member ID, or `SelectBarycentre`); a second click does nothing yet, as
  stars and pairs are not followed. Selecting the whole system rings its
  outermost pair's marker. The table shows only the primary's marker.
- Crowded markers give way, so a binary seen from afar is its cross, then
  its stars as they part, then their planets (`ui::canvas::shown`). By rank
  (barycentre, star, planet, craft), a marker is hidden while a shown one of
  higher rank is within `MARKER_SPACING` (24 points) of it; of two pairs'
  crosses, the outer pair's stays. Markers of one kind never hide each other.
  The selection, and so anything followed, is never hidden. The table and
  constellation views hide nothing, as they exist to show every body. A
  hidden marker has no ring, label, hover or click: zoom in, or use the
  breadcrumb or search.
- A click goes to the shown marker whose centre is nearest the pointer, then
  to the one nearer the camera (`Marker::depth`), and only that one has a
  hover ring (`ui::canvas::under_pointer`). A marker beats the star points
  behind it: galaxy picking runs only when no marker is under the pointer.
- Following a craft targets its blended position
  each frame (`craft::followed`), from 200 m. It uses the craft's retained body
  data, not visible-star reachability, and tracks system/galactic handoffs
  without starting another camera flight. Open-space following uses the craft's
  local galactic anchor for close-up precision. A followed craft in a system
  brings the camera to that system. One in galactic space leaves arriving and
  leaving to the camera's own proximity, as in free flight, so a ship warping
  in shows its destination from 0.02 ly out, not only once it slows below
  light speed and enters the system's frame. The craft is then drawn and
  followed from its galactic anchor, converted into the system's frame.
- Craft following retains the target-to-eye offset separately from the large
  local position. Both hull and label use that exact camera-relative centre,
  rather than adding a 200 m offset to a light-year-sized number and subtracting
  it again. This keeps high-speed warp rendering steady between physics steps.
  The craft also retains its previous displayed state and transforms it with
  every re-anchor/frame handoff. Camera velocity uses the resulting displayed
  displacement plus changes to the follow offset, divided by real frame time.
  Re-anchoring therefore does not produce a zero-speed frame or hide the velocity
  marker. Pausing still yields zero travel; camera-orbit marker rules are unchanged.
- Esc while following reframes the system and clears selection; otherwise it
  clears selection, or with nothing selected goes up physically. Leaving a
  system keeps either selection and stops tracking. Moving or looking stops
  following but not moving with the object
  ([moving with objects](#moving-with-objects)).
- `navigation/galaxy_nav.rs` picks and visits the selected star (`visit_selected`): a
  mapped list page (`galaxy_nav::mapped`) opens that page, any other opens its
  generated system. A new snapshot moves the visit with its star
  (`navigation::follow_visited_star`).
- Clicks not on a planet pick from loaded, visible points only, and only on
  click, never every frame. Picking traverses `StarCell::query_blocks` with a
  conservative sphere/cone test, then applies magnitude and angular tests only
  to leaf candidates rather than scanning every loaded star.
- Each click logs elapsed time and stars examined as `Star picking`.
  `Starfield::picked` caches the last ID and point so selection and inspector
  reachability avoid rescanning its cell. Replacement/removal of that cell or
  restart invalidates the cache; unrelated streaming changes retain it.
  Uncached typed ID lookups still scan their cell once.
- Click again or Enter to visit. The bodies drawn are the visited system's
  stars and planets. Search results can be charted for unstreamed visits;
  see [object search](../simulation/search.md#client-requests).
- Unassigned cell stars use ID-derived `TS` catalogue names and lettered planets,
  with no name registry or per-star UI entities.

## Camera frames and controls

- Use f64 for physics and local camera positions. Subtract the camera before
  converting positions to f32; do not add interstellar offsets to GPU floats.
- `Navigation` holds the destination; `CameraRig` flies the displayed pose there.
  Discrete changes set `Navigation::fly`. `Navigation` and orbital positions
  use the page's plane axes; `scene::page_frame` rotates them into galactic axes,
  which `CameraPose` and star offsets use.
- Each page star is a cell star at its own `PageStar::position_ly`. Page changes
  use the same `Flight` as list jumps, not a page-swap animation.
- In free-look/free flight, `Navigation::eye` is the authoritative f64 position
  in the current page frame. `target` remains for orbiting and flights; never
  reconstruct a close-up eye by cancelling a distant target against range.
  Velocity uses differences of local eyes and ignores rounding residuals on
  frame switches. Right-drag measures the eye's orbit in the speed readout,
  but the direction marker uses only intentional keyboard travel.
- Free-look and roll update the local quaternion. Left-drag turns about current
  screen axes and repositions the target to keep the eye fixed; right-drag
  orbits the fixed target about those axes.
- WASD, R/F travel and Q/E roll ease through one `CameraMotion::thrust`
  (camera-axis travel as a fraction of `move_speed`, roll in `w`) over about
  0.15 s; speed changes and mouse turns stay instant.
- Vertical field of view is `Navigation::fov_deg` (30° to 130°, default 50°),
  set in the settings tab (`UiCommand::SetFov`) or Shift + left-drag (up narrows).
  Hand projections (picking, overlays, table framing) use
  `Navigation::tan_half_fov`, never a fixed 50°.
- Shift + scroll (either Shift key) zooms from 1 m to 40,000 ly, whatever is
  followed or in view; plain scroll adjusts movement speed independently.
  Below 20,000 km the render scale floor (`scene::scale_floor`) comes down
  with the zoom, so what is zoomed on stays past the near plane.

Render scaling, floating-origin precision and depth are described in
[realistic rendering](rendering.md#precision-scale-and-wireframes).
