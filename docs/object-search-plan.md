# Object search · implementation plan

Status: Sessions 1 and 2 done (registry, query, engine, relationships,
native worker, client wiring, developer API and CLI; see
[galaxy model](galaxy-model.md#object-search) and `AGENTS.md`). Session 3
(UI kit and object browser) in progress; Session 4 (the browser's search
worker) to do. Fold this note into the design notes as each part lands.

## Goal

- One search system shared by four front ends:
  - the in-game object browser (prototype:
    `tools/prototypes/star-browser-sleek.html`),
  - the `ns` CLI and developer API,
  - a future guided query builder,
  - a future ship's navigator console.
- Every object kind is searchable, and new kinds can be added without UI
  changes.

## Principles

- **The query is the product.** Front ends only edit and display a `Query`;
  none of them knows object types directly.
- **The registry is data.** Kinds and properties are declared once; the UI,
  CLI and builder read them.
- **A scope is required.** Searches run around an origin within a radius
  (at most 1,000 ly), never over the whole generated galaxy. Exact
  designation lookups are the one exception: they need no scan.
- **Complete and deterministic.** Results never depend on the magnitude
  limit, streaming progress, thread count or which front end asked. The same
  query from the same origin gives the same results. A search stopped by its
  work cap says how far out it is complete.
- **No frame stalls.** Searches run off the main thread with a work budget,
  and candidates are identified by numbers (cell, component, index), not a
  `String` each.

## Architecture

### 1. Object registry

Lives in `crates/simulation`, shared by native, WASM and the CLI.

- **Kind:** ID (`star`, `planet`, `moon`), label, browsing family, parent
  kind, supported properties, its source (below), and whether it is
  available (has a generator).
- **Property:** ID (`radius_km`), label, unit, value type (range, choice or
  yes/no) and the kinds it applies to.
- **Families** are for browsing only, not scientific taxonomy: Stars &
  remnants, Planetary bodies, Moons & satellites, Small bodies, Systems, and
  Clusters, nebulae & galaxies. A query can name a family in place of kinds.
- Kinds without a generator are registered but hidden.
- Types within a kind are properties, not kinds: an "icy moon" is
  `kind=moon composition=ice`, a red dwarf is `kind=star star_class=m`
  (O, B, A, F, G, K, M, giant, white dwarf, as `star.rs` draws them), a
  sub-Neptune is `kind=planet planet_class=sub_neptune` (`system.rs`
  `Class`). The UI may still label choices plainly ("M · red dwarf").
- `ObjectRef` moves from `crates/client/src/objects.rs` into the simulation
  crate, so the engine can return it; the client keeps its hierarchy and
  inspector code.

### 2. Query model

Serde, versioned.

- `Query { text, kinds, filters, relations, scope, sort, limit }`
- `kinds`: a set of kinds, or `family` in their place; neither means any
  available kind.
- **Result granularity:** `kind=star` gives one result per star, so each
  matching member of a multiple system is listed, with its system as
  context. `kind=system` gives one result per system.
- `filters`: a property, then a range or a set of values ("is one of").
  All filters must match (AND). There is no OR or NOT across filters; a
  per-filter `not` flag can be added later without changing the shape.
- `relations`, each holding its own filters, nested as needed:
  - `Orbits`: the direct host (star, barycentre, planet, black hole). A
    barycentre host matches when **any** of its member stars matches, so
    `orbits.star_class=m` finds planets circling a pair of red dwarfs
    (`key#AB/n`) as well as a single one.
  - `SystemContains`: anything elsewhere in the same system, or the system
    itself. Its text prefix is `system.` (`system.planet_class=gas_giant`);
    `system=` without a dot is still the scope.
  - Each relationship is given once, one level deep; nesting
    (`orbits.system.`) is left for later.
- `scope`: `Within { radius_ly }` (at most 1,000 ly) or `System { id }`.
  The origin is not part of the query, so the same query (and its `ns`
  text) works from anywhere: `SearchRequest { id, query, origin, work_cap }`
  carries it. It is fixed when the request is sent: a position today (the
  camera), later an entity such as the ship. It does not follow the camera
  afterwards.
- `sort`: distance from the origin only, ties broken by ID, so nearest-first
  searching can stop early. Sorting by a property is allowed only for
  `System` scope, or later for a search that finished its whole scope.
- `limit`: the number of results wanted; with nearest-first this also makes
  large radii cheap ("the nearest white dwarf within 1,000 ly").
- `text`: an exact catalogue designation or ID (`TS 1abc-2-34`,
  `g1abc.2.34/3`). Names are derived from IDs, so this resolves directly,
  without a scope or scan. There is no partial name matching of generated
  objects, and list names and `System n` list stars are not searchable yet.
- One text syntax for the CLI, converting to and from the query exactly:
  `search kind=planet planet_class=sub_neptune orbits.star_class=m,white_dwarf within_ly=50`.
  Sets are comma-separated, ranges use `..` (`radius_km=1000..5000`, open
  ends allowed), and each property has one unit, the registry's.
- Unknown properties, or properties that do not apply to the chosen kinds,
  are refused with a reason, as `ns cmd` refusals are today. With several
  kinds, a property must apply to at least one; kinds it does not apply to
  are excluded by that filter.

### 3. Search engine

A service, not part of any UI.

- A request is `SearchRequest { id, query }`; results come back tagged with
  the same `id`, so several queries can run at once without overwriting each
  other. A front end that replaces its query (a chip edit) cancels its
  previous request; cancelled work stops at the next budget check.
- **Own data, not the streamed sky.** Streamed cells hold only stars bright
  enough to see (at magnitude 6.5, a typical red dwarf streams within about
  7 ly), so searching them would miss most of a scope and change with the
  magnitude limit. The engine has its own `Sky` cache and covers every
  tier's cells that overlap the scope sphere, regardless of brightness.
- **Nearest first.** Coarse cells (32 pc and up) come off a queue nearest
  first and each one's subtree is walked depth first, so the first `limit`
  results sorted by distance are final as soon as the next queued cell is
  farther, and a dense scope (the galactic centre) can stop early. Strict
  nearest-first order down to the smallest cells was tried: at 1,000 ly its
  queue held millions of cells and the cache reached 4.4 GB.
- **Work cap.** Each request has a total work cap (default 100 million
  units: stars tried plus 32 per cell, about 0.2 µs each natively) as well
  as a per-slice budget. A search stopped by it reports the radius it is complete to
  ("searched out to 12 ly"); results inside that radius are still exact.
- **Sources per kind.** Each kind's registry entry names a source that lists
  its candidates in a scope: cell stars and their systems today; later a
  moon, small-body, nebula or galaxy source. The engine asks the sources the
  query's kinds need and merges their results. Within the cell source, work
  runs cheapest first:
  1. stars in the scope's cells, by position only,
  2. star and system filters,
  3. generate planets (later moons) only for the systems that remain,
  4. relationship filters.
- **Cost rules for the cell source** (see Performance below):
  - Walk the cell tree from the root, skipping subtrees that miss the
    sphere or hold no stars of the tiers below them (`Sky::counts`), rather
    than listing every overlapping cell of each tier: a 50 ly sphere spans
    about 900,000 tier-19 cells, nearly all empty.
  - Place stars without their light first (`Sky::try_place_if`). A star's
    position is a descent through child octants; stop it as soon as its
    cell leaves the sphere, so most stars of a large bright-tier cell are
    rejected in a few random draws, without `tiered_light`.
  - Generate a system only for stars inside the sphere and nearer than the
    current last result; planets only when a kind or filter needs them.
  - Never keep generated stars, only results. Trim the search `Sky` cache
    (`Sky::retain`) to coarse cells ahead of the walk and the current
    branch, so memory stays bounded however dense the scope.
- Natively, `SearchService` runs every open request in turn on one thread,
  a slice each, and shares large cells' stars among several threads (as the
  star loader does). In the browser, a dedicated search Web Worker runs the
  same `Search` with one thread, separate from the star worker (which is
  replaced on every magnitude change and must not be slowed by searches).
  Partial results stream with "N searched, more available".
- **Results:** `ObjectRef`, a few display facts and the distance. No UI
  types, so egui, CLI JSON and a 3D ship console can each render them.
- Metrics in the perf tab and the snapshot, as fixed-size numbers only:
  stars placed, stars rejected early, cells walked, cache size and time per
  request.
- Legacy list pages without a cell star have no position in the galaxy, so
  radius searches never find them.
- Planned rule, not built yet: `known_by: <entity>`, for searches limited to
  what a ship's sensors have detected.

### 4. CLI and API

- Every search is stored by its request ID. The CLI's searches never change
  what the in-game browser shows.
- `POST /search` (query as JSON or text, with an optional origin and work
  cap) returns a request ID; `GET /search/<id>` returns its results and
  progress, `POST /search/<id>/cancel` stops it and `GET /search` lists
  them. Only the last 32 requests are kept.
- `UiCommand::Search { query }` opens the browser with that query and
  `UiCommand::ClearSearch` clears it, each with a `command_line::USAGE`
  entry. These are the only way the CLI drives the browser.
- `GET /catalogue`: kinds, families and properties from the registry.
- `ns search ...` (waits for the result, `--json` for machine output),
  `ns search --show ...` to send it to the browser instead, and
  `ns catalogue` to discover filters.

### 5. egui browser

- A new, third halo icon at the top right opens it; the Tasks window
  stays as it is, with its own task search.
- Follows the prototype: search box, an **Add filter** picker driven by the
  registry (types, then properties, then relationships), chips, scope and
  results list. Drop the prototype's name and type sorts.
- Distances are labelled "from where you searched", with a **Refresh**
  action that resends the query from the current position.
- Clicking a result inspects it; its actions reuse `visit_star` and
  `fly_to_planet`.

## Performance

The risk is not frame drops (searches never run on the main thread) but
time to results, memory, and CPU taken from star streaming. The galactic
centre is the worst case: its stream benchmark loads about 115 million
stars, and a scope there overlaps the same large bright-tier cells.

Measured on the engine with
`cargo test -p space-simulation --release search_probe -- --ignored --nocapture`
(`crates/simulation/src/search_probe.rs`): release, no work cap, 1 thread
and 20. "Nothing" has a filter no star passes, so the whole sphere is
searched; "sub-Neptunes" is `kind=planet planet_class=sub_neptune`, limit 50.

| Where | Radius | Query | 1 thread | 20 threads | Peak cache |
|---|---|---|---|---|---|
| Home | 50 ly | nothing | 0.28 s | 0.26 s | 19 MB |
| Home | 50 ly | sub-Neptunes | 0.23 s | 0.22 s | 1 MB |
| Home | 1,000 ly | nothing | 76 s | 76 s | 95 MB |
| Home | 1,000 ly | sub-Neptunes | 0.27 s | 0.26 s | 1 MB |
| 500 ly from centre | 50 ly | sub-Neptunes | 3.4 s | 0.8 s | 1 MB |
| Centre | 5 ly | nothing | 24 s | 3.8 s | 4 MB |
| Centre | 50 ly | sub-Neptunes | 21 s | 2.8 s | 1 MB |

- Near home everything is well under a second. The 0.2 s floor is the
  first coarse cells' masses, worked out once per search.
- At the centre the cost is the bright tiers' huge cells, which overlap any
  sphere there: about 145 million stars to place whatever the radius.
  Threads cut it about seven times natively; the browser has one.
- The slowest case is a complete 1,000 ly scan near home: 10 million small
  cells, each needing its children's counts, one by one. Threads do not help
  yet. The default work cap stops it at about 20 s with "searched out to".
- Memory stays under 100 MB in every case.

Optional later optimisations, each strapped on without changing results:

- **Reuse streamed cells.** Tiers whose streaming reach covers the scope
  could come from the streamed sky instead of being placed again. Removes
  most of the centre's cost near the camera. In the browser it needs cells
  passed from the star worker to the search worker (a copy, or shared
  memory with cross-origin isolation headers), so it waits until centre
  searches matter in practice.
- **Subtrees in parallel.** Walk several coarse cells' subtrees on separate
  threads, each with its own `Sky`, for complete wide scans natively.

## Session 1 · Measurement and engine (done)

1. Performance measurement; the cost rules held, with the walk order
   changed (above).
2. Registry, query types and text syntax, with tests.
3. `ObjectRef` moved to `space_simulation::objects`; the client re-exports
   it.
4. Engine, native `SearchService`, budgets, work cap, cancellation and
   counters. Tests: results match a brute force near home for several
   queries and every property; identical with any thread count or budget
   slicing; a capped request is exact within its complete radius; designations
   and IDs find their objects; a system scope sorts by a property; the
   service answers, refuses and cancels.

Not done in Session 1, moved to Session 2 where the client first sends
requests: wiring `SearchService` into the client, its counters in the perf
tab and snapshot, and the frame-drop check at the centre (all done there).

## Session 2 · Relationships, CLI and API (done)

1. Client `search.rs`: `Searches` owns the native service (a quarter of the
   machine's threads), shared with the API's thread, keeping the last 32
   requests by ID.
2. Relationships: `orbits.` (barycentre hosts match when either star does)
   and `system.` across stars, systems and planets, in the registry, query,
   text syntax and engine.
3. `POST /search`, `GET /search`, `GET /search/<id>`,
   `POST /search/<id>/cancel`, `GET /catalogue`; `search`, `clear_search`
   and `set_search` commands; `ns search` (also `--show`), `ns searches`
   and `ns catalogue`; `scripts/test-ns.py` extended.
4. Debug: a log line per request start, refusal and end (`search` target);
   the `perf` tab's `search` section (each request's counters, threads and
   work cap with buttons); the `state` tab's `search` heading; the
   snapshot's `search` object.
5. Tests: the acceptance query matches a brute force over every system
   within 50 ly of home (the nearest 1,000 of them); relationships match an
   independent check of each system; a pair host matches through either
   star; the store, HTTP routes, command parsing and CLI.

Left for the user, in their own window: the frame-drop check at the
galactic centre while a search runs (`ns perf`).

## Session 3 · UI kit and object browser

1. **UI kit** (`ui/kit/`), extracted from the debug console rather than
   designed up front:
   - `Theme` values (colours, fonts, sizes): `Theme::TERMINAL` for the
     debug console and `Theme::GAME` for player-facing windows.
   - Shared interaction (hover and held-press light), the floating window
     (header drag, opens below the halo icons), halo text (from
     `breadcrumb.rs`), facts, notes, text input, buttons and the widgets
     the browser needs: chips, search box, menus and result rows.
   - `ui/terminal.rs` stays the debug console's text-mode skin over the
     kit; the console must look and behave the same. Tasks and the
     inspector move over when they are next changed.
2. **Results carry their system's position** (`SearchResult::position_pc`),
   and navigation keeps the shown request's stars as charted, so a result
   can be visited without being streamed (locating a star from its ID
   alone takes about 150 ms).
3. **Object browser** (`ui/browser/`), a third halo icon. The prototype is
   a loose guide; no object pictures or 3D previews yet.
   - Search box: an exact designation, or pasted `ns search` text, which
     becomes chips.
   - **Add filter** picker driven by the registry: kinds by family,
     properties for the chosen kinds, then relationships; set, range and
     yes/no editors. Each filter is a removable, editable chip.
   - Scope (radius or the visited system) and limit; progress, "searched
     out to" and **Refresh** (resend from where you are).
   - Results list (name, system, class, distance); selecting one shows its
     inspector facts and actions (fly to, inspect), plus copying the query
     as an `ns search` command.
   - `UiCommand::Search` (and `ns search --show`) opens the browser with
     its query.
4. Update `scripts/test-browser.mjs` (icon positions included), `AGENTS.md`,
   `docs/galaxy-model.md`, `docs/controls.md` and the project structure.

Done when the same query built in the UI and typed in the CLI gives
identical results. Until Session 4 the browser build's window explains that
searching runs in the desktop app.

## Session 4 · Browser search worker

1. Search Web Worker, built by `scripts/build-web.sh`, with a fallback like
   the star worker's. Lift the browser's refusal of `search` in
   `UiCommand::check` and give `Searches` a worker in place of the hub.
2. Measure the one-thread browser timings; the browser may need its own
   default work cap.

## Later

- **New object kinds:** moons, asteroids and comets, then clusters, nebulae
  and galaxies. Each needs a generator, a source and a registry entry; the
  query, engine and UI stay unchanged.
- **New stellar populations:** brown dwarfs, neutron stars, black holes and
  rogue planets change galaxy generation itself (today neutron stars and
  black holes are redrawn as stars). They must keep generation draw order and
  component indices, so each is a generation project first, then a search
  source.
- **Per-filter `not`:** "rocky planets not in a multiple system".
- **Property sorts** over a search that finished its whole scope.
- **List search:** list names and `System n` list stars in the search box.
- **Saved searches:** stored `Query` values.
- **Guided query builder:** a sentence-style editor over the same `Query`
  ("Find **sub-Neptunes** orbiting **red dwarfs** within **50 ly**"), with
  each part editable.
- **Ship navigator:** an interior console that sends a `SearchRequest` with
  the ship as origin and its sensor range as radius, optionally limited by
  `known_by`.

## Decisions made

- **Data:** the engine loads its own complete scope, not the streamed sky.
  Reusing streamed bright tiers is an optional later optimisation.
- **Walk order:** coarse cells nearest first, subtrees depth first.
- **Threads:** several natively for large cells; one in the browser.
- **Work:** counted in stars and cells, never time; a cap per request.
- **Origin:** in the request, not the query.
- **Scope:** within a radius of the search origin (at most 1,000 ly, with a
  limit and work cap), or one system; no pre-built galaxy-wide index.
- **Origin fixed** when the request is sent; the browser offers Refresh.
- **Sort:** distance only, ties by ID.
- **Types are properties:** `star_class` and `planet_class`, not kinds.
- **Systems:** a single star is listed once, as a star; a multiple's system
  result is separate from its members.
- **Multiple systems:** one result per star; `kind=system` for one per
  system.
- **Orbits:** direct host; a barycentre matches if any member star does.
- **Relationship text:** `orbits.` and `system.` prefixes; one of each per
  query, one level deep.
- **API requests** are separate from the in-game one; the CLI drives the
  browser only through `search` (`ns search --show`).
- **Search threads:** a quarter of the machine natively by default, since
  the star loader takes three quarters; `set_search` changes it.
- **Browser worker:** a dedicated search worker, not the star worker.
- **Extensibility:** a source per kind, not a fixed star-to-planet pipeline.
- **Query logic:** AND across filters, sets of kinds and of values; no OR.
- **Text search:** exact designations and IDs only.
- **Tasks:** stay in the Tasks window with their own search.
- **CLI and UI:** results stored per request ID; superseded requests are
  cancelled; the CLI changes the browser only through
  `Search`/`ns search --show`.
- **Where the browser lives:** a new, third halo icon.
- **UI kit:** one module of themed widgets inside the client, extracted from
  the debug console; two themes (terminal, game). Not a separate crate.
- **Browser look:** the prototype's layout as a guide, not a specification;
  text only, no object pictures yet.
