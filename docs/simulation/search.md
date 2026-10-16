# Object search

The registry, query model and search engine live in `space_simulation::search`.
The engine searches procedural objects independently of visible-star streaming.
The [object browser](../client/ui/browser.md) and
[developer API/CLI](../client/developer-api.md#object-search) share the same query.
The [original plan](../plans/object-search.md) retains implementation history,
measurements and future ideas, not the current UI specification.

## Registry and query

- `registry` declares kinds (`star`, `system`, `planet`, and `ship` in the
  Craft family) and properties once. Later kinds are registered but hidden
  until they have a source: `Cells` for generated objects, `Live` for
  objects only the client holds (see [live objects](#live-objects)). Families
  are browsing groups, not scientific taxonomy.
- Types within a kind are properties: `kind=star star_class=m` for a red dwarf,
  `kind=planet planet_class=sub_neptune` for a sub-Neptune. Stellar classes
  include O, B, A, F, G, K, M, giant and white dwarf.
- `query::Query` is serde/versioned, with `text`, `kinds`, `filters`,
  `relations`, `scope`, `sort` and `limit`. `Query::parse`/`to_text` provide
  one text syntax; `check` refuses unsupported queries with reasons.
- Kinds can be a set or a family; neither means any available kind. Filters
  combine with AND, while a choice filter accepts any of its selected values.
  There is no OR/NOT across filters, no per-filter negation and no nested relations.
- Scope is `Within { radius_ly }` (positive, at most 10,000 ly) or
  `System { id }`. Defaults are 50 ly and 50 results; limits are 1 to 1,000.
  Distance sorting is nearest-first with ID tie breaks; one-system searches
  can also sort by a property.
- `text` is an exact catalogue designation or ID (`TS 1abc-2-34`,
  `g1abc.2.34/3`). ID-derived names resolve directly without a scope or scan.
  Partial generated-name matching, list names and `System n` list stars are
  not searchable yet.
- Example: `kind=planet planet_class=sub_neptune orbits.star_class=m,white_dwarf within_ly=50`.
  Sets are comma-separated; ranges use `..` (`radius_km=1000..5000`, open
  ends allowed). Every property has one unit, supplied by the registry.
- Unknown or incompatible properties are refused with reasons. With several
  kinds, a property must apply to at least one; incompatible kinds are excluded
  by that filter.
- `kind=star` returns individual matching members of multiple systems;
  `kind=system` gives system-level results. Results carry `objects::ObjectRef`,
  display facts, distance and `SearchResult::position_pc`, not UI types.
- A `SearchRequest` adds a fixed origin in parsecs and a work cap. The origin
  is not part of the reusable query and does not follow the camera afterwards.

## Relationships

- `registry::RELATIONS` supplies relationship filters such as
  `orbits.star_class=m`, `system.planet_class=gas_giant`, `system.kind=planet`.
  All filters for one relationship must hold for the same related object.
- `orbits` leads from a planet to its direct host star, or each star in its
  host pair (`Hosted::host_letters`). A pair matches when either star does.
  From a craft it leads to its [orbit reference](craft.md#orbit-reference),
  a planet, a star or each star of a pair, so
  `kind=ship orbits.planet_class=gas_giant` works. Planets never orbit
  planets, so `orbits.kind=planet` narrows results to craft.
- `system` leads to every other star and planet in the same system, and to
  the system itself (`system.multiple=yes`), never to the object itself.
  `system=` without a dot is system scope, not a relationship.
- Each relationship appears at most once, one level deep. Nested relationships
  and sensor-limited `known_by` searches remain future work.

## Search walk and completeness

- `Search` (`search/engine.rs`) owns its own `Sky`, never the streamed one,
  so results do not depend on magnitude limit or streaming progress. Legacy
  list pages without a cell star have no galaxy position and cannot appear
  in radius searches.
- Coarse cells (level 12, 32 pc, and up) come off a nearest-first queue;
  each subtree is walked depth first. Stars are placed by `Sky::try_place_if`,
  which drops a star as soon as its descent leaves the sphere, before its light.
  Empty or non-overlapping subtrees are skipped.
- Only stars inside the sphere and nearer than the current last result have
  their system generated (`tiered_stars`, or `tiered_bodies` when planets are
  needed). Planet generation and relationship filters follow cheaper tests.
  The search stops once `limit` results are nearer than the next queued cell.
- Work is stars tried plus 32 per cell (about 0.2 µs each in recorded native
  measurements), never time. Results are identical however sliced or threaded
  (tested against brute force over overlapping cells near home).
- The default total cap is 100 million work units, separate from per-slice
  budgets. A capped search reports `complete_to_ly`: results nearer than that
  are exact; do not assume every partial result is final.
- Generated stars are not retained, only results. The cache is trimmed to coarse
  cells ahead of the walk and the current branch, keeping it near 100 MB even
  in the recorded 1,000 ly measurements. Historical performance and alternative
  walk-order experiments are in the [plan](../plans/object-search.md#performance).

## Execution services

- `Runner` (`search/runner.rs`) runs open requests in turn, a work slice each,
  taking `Order`s and giving `SearchUpdate`s. Clocks are the caller's, so it
  builds for every target. Cancellation stops at the next budget check.
- Native `SearchService` runs on one service thread, a million work units per
  slice, with several threads for large cells. By default search gets a quarter
  of the machine's threads; star loading gets three quarters.
- A dedicated browser Web Worker (`web/search-worker.js`, `SearchWorker` in
  `crates/sim-worker`) uses the star worker's WASM in its own instance, one
  thread, with orders/updates as JSON. It is separate from the star worker,
  which is replaced on magnitude changes.
- If that worker fails, the client runs searches in 4 ms frame slices and
  restarts unfinished requests. The perf tab and snapshot report search `mode`.

## Live objects

Craft are live state, not generated from seeds, so the engine never finds
them (`search::live`).

- A query naming only live kinds (`kind=ship`, `family=craft`) finishes in
  the engine at once without walking any cells.
- The client builds its craft as `LiveObject`s only when a request is sent,
  on the main thread (`search::live_now`; the developer API queues its
  requests there, as it does commands): kind, name, altitude and speed relative to the body it is around (the
  `altitude_km` and `speed_kms` properties), that body's ID for `orbits`,
  and its system's key and position.
- When a request is sent, `live::matching` tests those objects against the
  same plan: kinds, filters, relationships (regenerating the orbited body's
  system for its facts) and scope. A craft's distance is its system's. Text
  matches a craft's ID or, ignoring case, its name (`Ship 1`); such a request
  is answered at once without the engine, as craft names are not catalogue
  designations.
- `live::merge` merges the matches into every engine update, nearest first,
  then cuts to the limit. Sorted by a property, the side that property
  belongs to comes first, as no property applies to both craft and
  generated objects.
- Matches are taken once, when the request is sent: a request does not
  follow craft that move or appear afterwards.

## Client requests

- Client `search/` owns the service (`web.rs` in the browser). `Searches` owns
  a `Hub`, shared natively with the HTTP thread, keeping the last 32 requests
  by ID. Several requests can coexist without overwriting one another.
- `UiCommand::Search { query }` searches from the camera and becomes
  `Searches::shown`, cancelling the previous shown request. `clear_search`
  clears it; `set_search threads=<n> work_cap_millions=<m>` changes debug
  settings (threads are native-only).
- Shown results' systems are charted in `Navigation::charted`, allowing visits
  without streamed cells. Locating a star from its ID alone was measured at
  about 150 ms, so the result carries its system position.
- API requests never change the browser's shown request; `ns search --show`
  uses the application command path when that is wanted.
- Requests log start, refusal and end (state, counts, time) under `search`.
  The perf tab shows request counters/settings; the state tab shows the
  browser request; the snapshot's `search` object includes both.

New kinds extend the registry, source, `ObjectRef`, hierarchy and inspector
resolution rather than adding a separate UI catalogue. See the
[browser extension guide](../client/ui/browser.md#extending-the-browser).
