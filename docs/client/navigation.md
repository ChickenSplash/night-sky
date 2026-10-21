# Navigation, selection and camera frames

Physical location, the open list, selection, inspection and presentation are
separate concepts. See [controls](controls.md) for user actions,
[the scene interface](ui/interface.md) for breadcrumbs and inspection, and
[the information overlay](../shared/information-overlay.md) for list pages.
Client paths below are relative to `crates/client/src/` unless prefixed with `crates/`.

## Visits and presentation

- There is one mode: the streamed galaxy. List pages are ordinary cell stars,
  drawn and picked like any other, with no markers of their own.
- `Navigation::visit` (`navigation.rs`) is the system the camera is anchored to
  (at, flying to, or showing as a constellation): `Visit::List` (a list index,
  at `Navigation::page`) or `Visit::Generated` (task-free planets built from
  the star's ID, never added to the lists' `Universe`). `None` is open space,
  in galactic axes relative to a floating origin, `Navigation::open_origin_ly`.
  A visit moves the camera frame to its star in f64, and the same `Flight`
  shifts the displayed pose.
- Open space's origin is the star just left, a followed craft's origin, or
  home for the galaxy reference views. Once the free-flight eye is more than
  `REANCHOR_METRES` (1e12 m) from it, `scene::locate` moves the origin to the
  camera. Measured from home, an eye thousands of parsecs out resolves only
  tens of kilometres, which would swallow slow keyboard steps.
- `presentation.rs` owns `PresentationMode`: `Simulation` (simulated orbital
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
- Planet location uses simulated f64 positions: enter within 20 physical
  planet radii, leave beyond 30. `scene::locate_planet` runs after camera
  placement, using the same simulation instant as the eye, including when
  following at high time rates. Render exaggeration, selection and following
  never affect this threshold; table/constellation layouts have no physical
  planet location.

## Selection and picking

- `Navigation::selection` is the one clicked object: `Selection::Star` (stable
  star ID), `Selection::Planet` (stable planet ID, including remote systems)
  or `Selection::Craft` (a live craft's ID, see [craft](../simulation/craft.md#in-the-client)).
  Selecting one replaces the other; `Navigation::select` keeps following only
  from planet to planet. A second click activates it: a star is visited, a
  planet or craft followed. Following a craft targets its blended position
  each frame (`craft::followed`), from 200 m. It uses the craft's retained body
  data, not visible-star reachability, and tracks system/galactic handoffs
  without starting another camera flight. Open-space following uses the craft's
  local galactic anchor for close-up precision. Camera-driven automatic system
  entry/exit is suspended while following a craft; its physical proximity still
  determines arrival and body visibility.
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
  system keeps either selection and stops tracking.
- `galaxy_nav.rs` picks and visits the selected star (`visit_selected`): a
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
  Free-flight velocity is the exact displacement the controls applied
  (`CameraMotion::moved_metres`), not a difference of eyes: hundreds of AU
  from a star an eye resolves centimetres, enough to swing the marker of a
  slow step by degrees. Flights and followed planets use differences of local
  eyes and ignore rounding residuals on frame switches. Right-drag measures
  the eye's orbit in the speed readout, but the direction marker uses only
  intentional keyboard travel.
- Free-look and roll update the local quaternion. Left-drag turns about current
  screen axes and repositions the target to keep the eye fixed; right-drag
  orbits the fixed target about those axes.
- WASD, R/F travel and Q/E roll ease through one `CameraMotion::thrust`
  (camera-axis travel as a fraction of `move_speed`, roll in `w`) over about
  0.15 s; speed changes and mouse turns stay instant.
- Vertical field of view is `Navigation::fov_deg` (30° to 130°, default 50°),
  set in the render tab (`UiCommand::SetFov`) or Shift + left-drag (up narrows).
  Hand projections (picking, overlays, table framing) use
  `Navigation::tan_half_fov`, never a fixed 50°.
- Shift + scroll (either Shift key) zooms from 1 m to 40,000 ly, whatever is
  followed or in view; plain scroll adjusts movement speed independently.
  Below 20,000 km the render scale floor (`scene::scale_floor`) comes down
  with the zoom, so what is zoomed on stays past the near plane.

Render scaling, floating-origin precision and depth are described in
[realistic rendering](rendering.md#precision-scale-and-wireframes).
