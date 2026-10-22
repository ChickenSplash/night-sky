# Camera location, moving with objects and body markers

Agreed with the user on 2026-10-22, before flight-sim phase 11. Three parts
that share one idea: where the camera physically is decides what it moves
with and what the breadcrumb says, by the same rule a ship uses to choose
what it orbits.

## Goal

- Moving or looking round a followed planet never lets it race off along its
  orbit; flying to another body moves with that one instead.
- The breadcrumb Location and the camera's motion never disagree.
- Stars and barycentres of a loaded system can be clicked like planets.
- Crowded markers give way to the most important one, and a click goes to
  the marker nearest the pointer.

## Where the work stands

- **Done and checked live (checkpoint 1):** moving with objects, the one
  location rule (`location_body`, `Field::select_reference_floored`) and a
  steady velocity readout: open space re-anchors on the eye, and free flight
  reads exact velocities instead of differencing positions.
- **Done and checked live (checkpoint 2, `966ce64`):** star and
  small-cross barycentre markers, `Selection::Barycentre`, the CLI's
  `select_barycentre`, and member stars and pairs in the Selected trail.
- **Done and checked live (checkpoint 3):** marker priority and click
  priority (decision 4: `canvas::shown`, `canvas::under_pointer`,
  `Marker::depth`, `MARKER_SPACING` of 24 points, kept after the live check).
- **Added during checkpoint 3, at the user's request:** a Scene overlays
  switch (`Navigation::overlays`, `set_display overlays`) in the render tab,
  with the overlay switches nested beneath it, including a new Craft
  predicted paths switch and Wireframes over bodies moved from realistic
  rendering. Off, it hides everything drawn over the scene that is not in
  it; clicks still select.
- **Done (checkpoint 4):** navigation, controls, interface and debug console
  docs. The plan is complete; next is flight-sim phase 11.

## Decisions

### 1. One location rule

- `Navigation::location_body` is the body the camera is at, chosen by
  `Field::select_reference`, the function that picks a craft's orbit
  reference:
  - a planet within its Hill sphere;
  - else the innermost star or pair of a multiple system whose stable zone
    holds the camera (Holman & Wiegert, from the pair's masses and
    separation, so a star with no planets still has one);
  - else nothing below the whole system, which `arrived` already covers.
    A single star never adds a level.
- **A camera-only minimum for planets: 30 planet radii.** A close-in planet's
  Hill sphere can be smaller than the following distance (12 radii): about 7
  radii for a hot Jupiter at 0.05 AU. Ships keep the pure Hill sphere,
  because a ship that close really does orbit the star. The Location and the
  camera's motion still always agree, both using the camera's rule.
  - Implementation: an optional minimum passed to the reference search
    (or a camera wrapper round it), so the descent and margins stay one
    piece of code.
- **No minimum for stars.** A stable zone is typically 20 to 45% of the
  separation: about 60 star radii for Sun-like stars 1 AU apart, about 6 at
  0.1 AU. In a tight pair the pair is the honest location.
- **One margin everywhere:** leave at 10% past the zone
  (`SWITCH_MARGIN`), for planets, stars, pairs and the craft neighbourhood.
- **The breadcrumb** shows the chain down to the body: for example
  "… › System AB › B", or "… › System AB › B › b" at a planet of B.
  Esc goes up from any of them as from a planet.
- **The CLI** reads `location_body` (renamed from `location_planet`).

### 2. Moving with objects

- `Navigation::anchor` is, by the camera's position alone:
  - a craft within 1,000 km, the nearest first;
  - else the `location_body`: a planet, a star or a pair, so the camera moves
    with a star of a binary as it orbits;
  - else nothing: still in the system's or galactic frame.
- Following only centres. Moving or looking stops centring, never the
  motion. Fly to and Follow reach the neighbourhood, and that is what moves
  the camera with the object; selection decides nothing.
- A handoff changes speed at once. If it lurches visibly, ease it over
  about a second.
- The HUD's "Moving with" uses the breadcrumb's name for the body.

### 3. Star and barycentre markers

Only while a system is loaded (arrived), as planet markers are.

- **Stars:** a marker for every star on the page, single stars included, so
  a star in a multiple system can be clicked. The label is the breadcrumb's
  ("System 2 B"). Clicking selects the star (`SelectStar` with its member ID,
  which `selectable_star` already keeps), and the inspector shows its facts.
- **Barycentres:** a marker for every pair, the whole system's included, at
  its moving centre of mass. A distinct shape, such as a small cross, since
  nothing is there to see. Clicking selects it: a new
  `Selection::Barycentre(id)`, mapped to the existing
  `ObjectRef::Barycentre`, whose inspector facts the breadcrumb already
  shows.
- **A second click** on a selected star or barycentre does nothing for now:
  only planets and craft can be followed.
- Markers obey the markers and labels toggles as planet markers do.
- `Marked` gains `Star(id)` and `Barycentre(id)`; drawn in
  `scene/draw.rs` beside the planets, hit-tested in `ui/canvas.rs`.

### 4. Marker priority and click priority

**Crowded markers give way by rank.**

- Rank: barycentre, then star, then planet, then craft.
- Going from the highest rank down, a marker is hidden when a shown marker
  of higher rank has its centre within a fixed screen distance, about 24
  logical pixels (to tune live). Pair by pair, so two markers are enough.
- A hidden marker is gone: no ring, label, hover or click. Zooming in brings
  markers back as they spread apart: a binary seen from afar is its cross,
  then its stars, then their planets.
- Within a rank nothing hides, except pairs: of two barycentre crosses
  together, the outer pair (nearer the whole system) stays. Planets, stars
  and craft stay side by side as now.
- The selection and a followed object are always shown. They hide lower
  ranks near them as their rank does, and are never hidden themselves.
- No hiding in the table or constellation views, which exist to show every
  body side by side.
- No "+n" count of hidden markers for now: zooming in or the breadcrumb and
  search reach them.
- Trade-off accepted: zoomed out to a whole system, inner planets near
  their star cannot be clicked until the camera is closer.

**Clicks go to the nearest shown marker.**

- **Now:** any marker beats the star points behind it: a click falls
  through to star picking only when no marker is under the pointer. Among
  overlapping markers, though, the first drawn wins (planets, then craft,
  then stars and pairs), and every overlapping marker shows a hover ring.
- **Change:** of the shown markers under the pointer, the one whose centre
  is nearest wins; on a tie, the nearer to the camera (a depth on
  `Marker`). Only that marker shows a hover ring. Markers still beat star
  points.

### Deferred

- Picking one star of a multiple system from its sprites before the system
  is loaded, which today selects the system from between the stars.
- Following or flying to a star or barycentre.

## Checkpoints

1. **One location rule and moving with stars:** finish `location_body`, add
   the camera's planet minimum, `Anchor::Body` for stars and pairs, the
   breadcrumb chain, the CLI key, docs. Tests: Hill sphere edges and the
   minimum; a member star's zone in the demo's multiple system and its
   breadcrumb; moving with a star of a binary over time; a ship nearby
   still wins. User checks it live, then one commit with moving-with.
2. **Star and barycentre markers:** drawing, labels, selection,
   `Selection::Barycentre`, inspector, CLI selection. Tests: markers only
   when arrived, one per star and pair, selecting each.
3. **Marker priority and click priority** (decision 4), in `ui/canvas.rs`
   as pure functions over the frame's markers: which are shown, and which
   one is under the pointer. Tests: each rank hiding the next within the
   distance and not beyond it; the outer of two pair crosses kept; planets
   side by side kept; the selected and followed markers never hidden; no
   hiding in the table or constellation views; nearest-centre and
   nearer-camera clicks; one hover ring; a hidden marker never clicked; a
   marker beating a star point.
4. Docs (`navigation.md`, `controls.md`, `interface.md`), `./scripts/check.sh`,
   a live check by the user, then flight-sim phase 11.
