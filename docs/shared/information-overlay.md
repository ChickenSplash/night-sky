# Lists, pages and tasks

Night Sky is a space simulator first. Lists and tasks are an optional information
overlay on generated systems, currently supplied only by the built-in demo.
The ClickUp integration was removed; there is no live task integration.

## Planets are not tasks

- A `Planet` is a generated body with a stable host/orbit ID and an optional
  `Task`. Task data (status, assignees, URL) never lives on the body.
- Select and follow planets by stable body ID (`Selection::Planet(String)`);
  resolve a local index only against the visited system with
  `selected_planet(galaxy)`. Remote inspection resolves the encoded parent
  star without visiting or needing streamed meshes.
- Find tasks with `StarSystem::tasks`/`task_planet`. Task status and deadlines
  never change orbital elements. Orbital identity survives reordering and edits.
- Task slots are append-only per list, including when tasks disappear from a
  snapshot. Tasks fill stable generated planets, not the other way around.

## Pages and assigned stars

- Lists are constellations of system pages. A list page's legacy star key is
  `star_id(list_id, page)`.
- Task pages are filled in order from append-only slots; each page's capacity
  is `planet_count(assigned_star_id)` for pages mapped to cell stars, or
  `page_capacity(list_id, page)` for legacy pages. The slot's index within
  its page picks its body.
- A page's whole planetary system is generated at once. Every page with a
  task gets all its planets, tasked or bare; a list without tasks shows page 0's.
  `place` builds this from tasks and `shape` repeats it from a system's own
  tasks, on the server and again on the client, so fixtures need only slots.
- Page-to-cell-star assignments live in the snapshot's
  `StarSystem::page_stars`, from `assign_page_stars`. They are unique and
  immutable per list/page; include removed tasks' allocated slots when deciding
  how many pages need a star.
- Mapped pages use the cell star's ID, position, formation and plane tilt,
  so visiting that star from the galaxy shows the same planets, with tasks as
  an optional overlay. Assignment skips systems with no planets.
- Assignment uses `VISIBLE_MAGNITUDE`, independently of the client's adjustable
  magnitude limit. Legacy pages without cell stars have no galaxy position,
  so radius searches never find them.
- Each legacy page has its own solar-neighbourhood star. See
  [stars](../simulation/stars.md) and
  [planetary generation](../simulation/planetary-systems.md) for its physical model.

## Interface boundary

Tasks (`crates/client/src/ui/tasks.rs`, 01 lists and 02 task index, with the
data-source note) keeps lists and tasks apart as the future integration point.
The rest reaches them through `UiCommand`s. Panel-only choices (list/task sort,
status groups) live in `tasks::Tasks`, not `Navigation`. Explain the data source
(currently always the demo) in the interface.

Table layout preserves physical radii and never changes orbital elements.
Constellation view keeps the current page's frame, fits page stars from the
current angle and draws links without moving the stars. Each page has its own
orbital-plane tilt. See [navigation](../client/navigation.md) for visits and
presentation modes, and [the interface](../client/ui/interface.md) for inspection.
