# Scene interface, HUD and inspection

The egui interface (`crates/client/src/ui.rs`) draws in the same frame as
the scene. The 3D view fills the whole window: no sidebar or HTML
UI. HTML/CEF prototypes were abandoned (`prototype-cef-overlay`). See
[controls](../controls.md) for shortcuts, [navigation](../navigation.md) for
physical location, [the UI kit](kit.md) for shared widgets, and
[the browser](browser.md) for query editing. Client paths below are relative
to `crates/client/src/`.

## Windows and commands

- Four free-floating halo icons at top right (`ui::icons`, ⚙ then ✔ then 🔍 then ⓘ,
  `kit::halo::halo_glyph`, hover blue, white while open) toggle the debug
   console, Tasks, object browser and Object Inspector. They first open below the icons,
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

## Object Inspector window

The ⓘ icon or **I** key toggles a single, initially closed **Object Inspector** using the
object browser's shared player-window theme, blur, movement and resizing. It
is drawn as a compact circle, dot and stem rather than a font glyph, avoiding
missing-font fallback squares. The shortcut is inactive while typing. The window
follows ordinary selection only, without travelling or falling back to location.
The existing borderless breadcrumb readout is unchanged. Non-planet selections
reuse shared object facts; no selection shows a prompt.

Planets show the validated active result's ordered regions, not pending refinement
candidates. An interactive annular cross-section preserves inner and outer radii
relative to the outermost modelled region, including atmosphere extent. Colours
identify regions, not physical appearance. Thin layers are never exaggerated:
the exterior-to-centre overview keeps every region selectable even when its ring
is sub-pixel. Both views share local layer selection, reset when the object or
active record changes. Wide windows place them side by side; narrow windows stack
them. This is a radial model diagram, not a rendered surface or phase inference.

Layer details show material, radii, thickness, mass, boundary pressures in bar/mbar,
available temperature, model and fidelity. A composition breakdown lists every
nonzero material, largest first, with its mass and percentage of the selected
layer's mass (not the whole planet). Unresolved quantities stay unknown;
envelope/compression temperatures describe an outer boundary, not a resolved
profile. The window ends at the selected-layer stats and composition, without explanatory footers
or global model summaries. Full model limitations and reference-boundary facts
remain in the full shared data used by browser/developer readouts. Missing/invalid active records
show no invented layers.

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
  either trail uses the same selection commands as scene markers and browser
  rows, including ordinary galaxy and list selections. Matching crumbs highlight
  in both trails. The shared borderless `ui/inspector.rs` readout below follows
  selection, or current location with no selection. There is no separate
  inspection override. Repeated crumb clicks only select, never travel; tracking
  follows the normal selection rules.
- The inspector keeps its borderless halo text. Planet readouts show only type,
  mass, radius, gravity, and orbit/rotation facts, plus navigation actions. Layer,
  composition, atmosphere, boundary and refinement/model details are omitted from
  this scene overlay; the Object Inspector owns detailed interior inspection.
  A shared-kit vertical
  scroll area bounds long content to the viewport, reserving the bottom HUD
  inset; wheel/scrollbar input reaches all facts and actions. Its top stays
  anchored at the shared HUD inset. Facts are cached
   until target/snapshot/active physical revision changes; loaded planets use
   their actual active system rather than regenerating a remote fallback. Remote
   systems are never regenerated per frame.
- `ObjectRef::Star` by system key is the whole system: its star or outermost
  barycentre, labelled with all letters (`System 2 AB`). Below it, both trails
  show the planet's `StarSystem::lineage`: component star (`key#B`), and in
  triples/quadruples an inner pair (`ObjectRef::Barycentre`). A member's
  actions act on its whole system.
- Remote crumbs use `objects::system_letters` (stars only, cached) and the
  host encoded in the planet ID, never a whole generated system per frame.
  New object kinds extend `ObjectRef` (`space_simulation::objects`, re-exported
  by client `navigation::objects`), search registry, hierarchy builders and inspector
  resolution. Moons do not yet exist.
- `UiCommand::Up` is separate keyboard navigation: reframe tracking, else
  clear selection, else move up physically; crumbs contain no travel code.

## Inspector actions

The inspector presentation uses typed fields: stable planet field IDs, sections,
labels, display units, formatted known values or explicit unknown reasons. Both
halo rendering and developer export consume them; physical records remain separate.
The browser uses the same fields' label/value projection. Composition includes
retained material mass fractions (not atmospheric mole fractions), retained and
escaped mass, mean density, gravity, H₂/He fraction, gas/condensate reservoirs,
radius meaning and model caveats. Missing legacy composition and invalid new
records are visibly unknown, never zero-filled. Water-rich boundaries make no
ocean/phase claim; a one-bar radius is not ground or a measured cloud deck.

The full shared record retains Overview, Background refinement, Composition,
Atmosphere, Orbit and rotation, and Model details sections for browser/developer
consumers. The top-left readout filters this data for presentation only, without
deleting facts or changing API exports. Refinement facts distinguish unapplied
candidates, safety deferrals and applied active results; see
[the layered record](../../simulation/planet-composition.md#layered-transport-record).
Composition includes ordered regions and bulk gas inventories; these are
whole-planet mass fractions, not atmospheric molecular percentages. Atmosphere
facts retain mixture, estimated temperature and scale height. Reference-boundary
meaning, uncertain pressure, phase knowledge, fidelity and limitations remain in
the full data. An absent/sub-cutoff model has unknown pressure, never an invented
exact zero. The Object Inspector currently displays region details, not every
field of that full record.

Planet facts include generated all-star mean heating, equilibrium temperature
with assumed albedo, atmospheric composition, reference pressure, column
temperature/density, scale height and operational extent. Pressure displays
use bar, switching to mbar below 0.01 bar (scientific notation
for trace columns rather than rounding positive pressure to zero). Physical
records retain pascals. Estimates and unresolved escape/thermal structure are
labelled; supported spherical columns do not inherit the old extended-column
uncertainty warning. Supported condensed boundaries show
approximate boundary pressure and the one-bar altitude above it; composition
flags unresolved deep boundaries, which are not proof that ground is absent.
A giant's 1 bar reference level is not ground. Every gas column states its
ideal-gas, dilute-optics, greenhouse and phase limitations. These are simulation data, independent of debug atmosphere sliders;
they now drive each planet's atmosphere rendering unless that selected planet
has a debug override enabled. See [physical atmospheres](../../simulation/atmospheres.md).

Below facts, `inspector::actions` shows clickable halo text separated by `·`,
hover blue, `CURRENT` when already showing the view:

- Galaxy: **View from above**.
- List: **Constellation view**, **Previous system** / **Next system**, then
  a `System n · k tasks` line per page. These act like clicking the star
  (`PickStar`: select, or visit once selected; list stars resolve unstreamed
  through `page_pick`), relative to the selected star in that list, else the
  open list's page. Selecting a system replaces the list selection and the
  inspector follows that star, with no pinned list readout.
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
