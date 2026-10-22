# Ship controls schema · plan

A detour before [flight simulator](flight-sim.md) phase 9, agreed with the user
on 2026-10-22. Future-tense descriptions are proposals, not current behaviour.

## Goal

Ship orders and state grow every phase, and each addition was written by hand
in the simulation, `scripts/ns`, its tests, the docs and (as raw JSON) the
TUI. The Rust types become the single source of truth: a new order or state
field appears in the CLI and the TUI with no client change.

## Design

- **Derived, not hand-kept.** `schemars` derives JSON Schema from `Order`,
  `WarpOrder`, `PointTarget` and the state types. Doc comments are the help
  text; range attributes name the same constants validation uses, so the two
  cannot drift. Extensions add what JSON Schema lacks: `x-group` (sublight,
  turning, warp) and `x-unit`.
- **One normaliser, in Rust.** JSON Schema is awkward to walk, so Rust turns
  it into a flat, ordered catalogue that clients read without knowing JSON
  Schema:
  - each order: its words (`warp set`), group, help, the order JSON skeleton
    and its fields;
  - each field: key path, kind (number, boolean, text, choice, vector,
    variant), range, unit, default, and whether the CLI takes it as a
    positional argument or a flag;
  - each state field: path, group, unit and help, for display.
- **Served and committed.** `GET /craft/schema` serves the catalogue (raw
  JSON Schema is not served: nothing needs it yet). It is native only, like
  the API. A test writes the same catalogue to
  `scripts/craft-schema.json` and fails while the committed copy is stale, so
  `ns` works without the app and diffs show every control change.
- **Generated clients.**
  - `ns ship` builds its order subcommands from the committed file, keeping
    today's syntax. Spawning, listing, state, follow and despawn stay
    hand-written. `ns ship tree` prints the command tree.
  - `ns-tui` gains a **Ship** tab: a ship picker, grouped buttons (orders
    without fields run at once, others open a typed form) and a live state
    panel grouped by section with readable units, built from the live schema.
- **Rules for generation:** required fields are positional, in declaration
  order; defaulted fields are flags; nested structs become flags; a nested
  tagged enum becomes subcommands; `id` and `tick` from `steer_craft` are
  flags on every order. Tests require every order to have help and a group,
  and every numeric state field to declare a unit.

## API changes this brings

- `rotate` and `translate` accept omitted axes as zero, as the CLI already did.
- `ns ship thrust` sends the equivalent `thrust` order instead of
  `set_thrust`.
- Every order gains `--tick`, scheduling it for a world step.

## Checkpoints

1. ✅ Order schema, normaliser, `GET /craft/schema`, committed file and tests.
2. ✅ Typed ship state with groups and units, added to the schema. The
   readout's JSON is unchanged.
3. ✅ `ns ship` generated from the file; the existing CLI mapping tests pass
   unchanged apart from `thrust`. Adds `ns ship tree` and `--tick`.
4. ✅ `ns-tui` Ship tab, smoke-tested headless against the running app.
5. ✅ The user tries the TUI against a live ship; this led to timed orders.
6. ✅ **Timed orders**, below, built 2026-10-22: `--in` on every order,
   `--for` on throttle, translate, rotate, point, thrust and warp engage.
   Resolved in the app rather than by the client, and a later order on the
   same controls cancels a pending end. The user tried it live on
   2026-10-22. The detour is done; [flight simulator](flight-sim.md) phase 9
   is next.

## Timed orders

Trying the TUI showed that `tick`, an absolute world step (about 173 million
by then), reads like a duration. Orders hold until changed, so a timed burn
took two hand-scheduled orders. People and AI agents should say when and for
how long, in seconds, never work out step numbers.

- **`in <seconds>`:** start after a delay, rounded to whole 0.05 s steps from
  the craft's current step. It replaces `tick` in the TUI's forms; `--tick`
  stays in `ns` and the API for scripts that need exact steps.
- **`for <seconds>`:** on orders with an obvious off state (throttle,
  translate, rotate), schedule the matching off order (zero on the order's
  axes) when the time is up, so `ns ship throttle 0.5 --for 1` is a one-second
  burn. Combines with `in`.
- **Where (decided):** `steer_craft` takes `in_s` and `for_s`, and the app
  turns them into steps from the ship's own step and queues the order and its
  end in one call (`Voyage::timed_order`), avoiding a read-then-send race. The
  readout's `pending_orders` gives each order's `in_s`.
- **From the schema:** which orders take `for`, and their off order, should
  come from the Rust types (such as an `x-off` extension or a method on
  `Order`), not a client list.
- **Decided:** an order on the same controls, applied after a timed order
  began, cancels its pending end, so a later manual throttle is not cut by an
  earlier burn's end. Each timed order uses two of the queue's slots.
- **Not timed:** stop, release, cut, configure and the other warp orders
  have no rest state; timing `warp set`, `turn-rate`, `configure` or
  `gravity` would mean restoring the earlier value, which needs the end
  worked out when the order applies.
- **Done when:** `ns ship throttle 0.5 --for 1` and the TUI's `for` field
  each give a one-second burn, and the readout shows the pending end.
