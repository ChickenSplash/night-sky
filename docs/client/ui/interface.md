# Scene interface, HUD and inspection

The egui interface (`crates/client/src/ui/mod.rs`) draws in the same frame as
the scene. The 3D view fills the whole window: no sidebar or HTML
UI. HTML/CEF prototypes were abandoned (`prototype-cef-overlay`). See
[controls](../controls.md) for shortcuts, [navigation](../navigation.md) for
physical location, [the UI kit](kit.md) for shared widgets, and
[the browser](browser.md) for query editing. Client paths below are relative
to `crates/client/src/`.

## Windows and commands

- Three free-floating halo icons at top right (`ui::icons`, ⚙ then ✔ then 🔍,
  `kit::halo::halo_glyph`, hover blue, white while open) toggle the debug
  console, Tasks and object browser. They first open below the icons,
  right-aligned (`terminal::below_icons` for debug). Tasks and browser start closed.
- Widgets come from `ui/kit/`: `Theme::TERMINAL` for debug through
  `ui/terminal.rs`, `Theme::GAME` for player windows. Halo text lives in `kit::halo`.
- Every panel action is an ID-based `UiCommand` (`actions.rs`). Panels own
  local editing/sorting state, not independent selection or camera state.
- Body facts, presentation modes and system paging live in the floating
  breadcrumb inspector. There is no telemetry section, centre-star pick or C key.
- The object browser submits query changes as `UiCommand::Search`; any `search`
  command opens it on that query. Results reuse inspector facts/actions and
  chart shown systems for unstreamed visits. Tasks remain the separate
  [information-overlay interface](../../shared/information-overlay.md).

## Breadcrumbs and inspector

- `ui/breadcrumb.rs` is free-floating top-left text under any window, with no
  container. A soft dark halo (two translucent black rings, `HALO_EDGE` at
  1 px and `HALO_FADE` at 2 px) keeps it readable over discs/glow and vanishes
  on dark space. Reuse `kit::halo::halo_text`/`halo_wrapped_text` for scene
  text, or `halo_galley` for manually placed text such as canvas planet labels.
- `breadcrumb::trail` is physical location: `Milky Way`, the current star's
  list if any, the star, then `Navigation::location_body` with the stars and
  pairs above it (such as `System 2 AB › System 2 B › b`). An open list or
  destination is not a location until arrival. `selection_trail` independently
  shows the selected object's hierarchy, including remote planets. List stars
  are `System n`; other stars use catalogue names.
- Crumbs carry stable `objects::ObjectRef`s, never camera commands. Clicking
  either trail sends `InspectObject` and activates a highlighted tab in the
  shared borderless `ui/inspector.rs` readout below both trails. Selection
  automatically activates its leaf. Inspection changes neither selection nor
  camera/tracking. Missing tabs fall back to selected leaf, or current location
  with no selection.
- The inspector uses natural content height, with no scroll area, scrollbar
  or bottom-HUD height reservation. Long information uses the left side of
  the view; its top stays anchored at the shared HUD inset. Facts are cached
  until target/snapshot changes; remote systems are never regenerated per frame.
- `ObjectRef::Star` by system key is the whole system: its star or outermost
  barycentre, labelled with all letters (`System 2 AB`). Below it, both trails
  show the planet's `StarSystem::lineage`: component star (`key#B`), and in
  triples/quadruples an inner pair (`ObjectRef::Barycentre`). A member's
  actions act on its whole system.
- Remote crumbs use `objects::system_letters` (stars only, cached) and the
  host encoded in the planet ID, never a whole generated system per frame.
  New object kinds extend `ObjectRef` (`space_simulation::objects`, re-exported
  by client `objects`), search registry, hierarchy builders and inspector
  resolution. Moons do not yet exist.
- `UiCommand::Up` is separate keyboard navigation: reframe tracking, else
  clear selection, else move up physically; crumbs contain no travel code.

## Inspector actions

Below facts, `inspector::actions` shows clickable halo text separated by `·`,
hover blue, `CURRENT` when already showing the view:

- Galaxy: **View from above**.
- List: **Constellation view**, **Previous system** / **Next system**, then
  a `System n · k tasks` line per page. These act like clicking the star
  (`PickStar`: select, or visit once selected; list stars resolve unstreamed
  through `page_pick`), relative to the selected star in that list, else the
  open list's page. They resend `InspectObject`; `breadcrumb::holds` keeps
  the list tab while open so inspection stays on it during stepping/flights.
- Star: **Fly to system** / **Frame system**, **Table view** and, for list
  stars, **Constellation view** (`UiCommand::VisitStar`).
- Planet: **Fly to planet** (`FlyToPlanet`, visiting its star then following,
  like a second click).
- Craft: **Follow ship** (`FlyToCraft`, visiting its system first if needed).
  Craft facts are recomputed every frame rather than cached.

Stars are reachable by ID if mapped, visited, charted by the shown search, or
streamed. Unreachable actions are hidden, not disabled. The read-only
[developer inspector](../developer-api.md#inspector-readout) exports these
same facts/actions without selecting or travelling.

## Scene canvas

`ui/canvas.rs` draws marker rings, labels, the selected star's ring and the
velocity marker over the 3D view, and turns clicks on it into selections.
Which crowded markers it hides and which one a click reaches are described in
[navigation](../navigation.md#selection-and-picking). With Scene overlays
off it draws nothing, but clicks still select (see [controls](../controls.md#debug-tools)).

## Text HUDs

- `ui/hud.rs` places two centred lines at the bottom using the breadcrumb
  font/colours and `halo_text`: elapsed simulation time above actual rate
  (two decimals, seconds/minutes/hours/days/years per real second).
- Click rate to pause; drag horizontally for logarithmic adjustment from
  real time to 100 years/s (400 logical pixels for the full range). Dragging
  preserves pause state, with `· paused` beside the retained rate, or
  `· held by craft` while craft physics cannot keep up with it. There is
  no bottom panel, preset ladder or separate speed editor.
- Bottom-left `ui::hud::speed_readout` uses the same two-line layout: measured
  velocity above draggable set speed. Horizontal drag adjusts logarithmically
  from `scene::MIN_SPEED` to `scene::MAX_SPEED` (1 m/s to 10,000 ly/s), through
  `SetMoveSpeed`. A single click does nothing.
- Bottom-right **Magnitude limit** (`ui::hud::magnitude_control`) previews
  while dragging and applies on release. It restarts streaming; see
  [magnitude changes](../streaming.md#magnitude-changes) for limits and effects.
- Above it, right-aligned matching controls adjust Exposure and Dynamic Range
  Compression (`ui::hud::exposure_controls`). Exposure drag is 20 pixels per
  stop (-64 to +16); +1 doubles light, -1 halves it. Compression uses 400 pixels
  for 0 to 100%. Both apply continuously through `SetExposure`/`SetCompression`,
  capture the full drag, do nothing on click, are disabled in wireframe mode,
  and are not persisted. See [rendering](../rendering.md#exposure-and-compression).
- All text HUDs share `breadcrumb::HUD_PADDING` (20 logical pixels from
  viewport edges); movement, magnitude and time share `HUD_LINE_GAP` and
  bottom-line alignment.

`ui::pointer_captured` protects background-layer breadcrumbs/inspector and
draggable text, capturing the full drag outside bounds. Scene camera input and
canvas picking both use it so HUD interactions never also move/select the scene.
While held, hit testing uses the press origin, not hover position: camera drags
started outside HUD text continue uninterrupted when crossing it.
