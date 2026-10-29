# Shared UI kit

The client uses one egui interface over the 3D scene. The visual direction
is precision: compact controls, consistent spacing and restrained accent motion,
with no HTML overlay. Windows float over the scene or dock into one sidebar
beside it.
See [the object browser](browser.md) for its use in query editing and results,
and [the scene interface](interface.md) for HUD and inspector behaviour.

## Structure and ownership

Paths are relative to `crates/client/src/`.

| Module | Responsibility |
| --- | --- |
| `ui.rs` | UI composition, top-right window toggles and command dispatch |
| `ui/hud.rs` | Draggable scene HUD controls and pointer capture |
| `ui/canvas.rs` | Scene captions, markers and picking overlays |
| `ui/breadcrumb.rs` | Independent location/selection trails and selection-driven inspector |
| `ui/inspector.rs` | Shared object descriptions, available actions and cached facts |
| `ui/tasks.rs` | List/task filtering, sorting, status groups and navigation commands |
| `ui/browser/` | Query editing, registry-driven picker and selected-result view |
| `ui/terminal.rs` | Thin terminal-theme adapter used by `devtools/` |
| `ui/kit/` | Reusable controls, themes, interaction and layout animation |

`Navigation` remains authoritative for selection and travel. Feature panels own
only their UI state, such as unfinished query text, sorting and folded groups.
Actions leave panels as `UiCommand`s, never as direct camera mutations. The
inspector's facts/actions are shared by breadcrumbs, browser and developer
readouts; extending an object should extend that source, not duplicate it.

## Themes and geometry

- `Theme::GAME`: player-facing windows and modal controls.
- `Theme::TERMINAL`: debug console, with monospace text-mode controls and
  the same hairline border colour and thickness as `Theme::GAME`.
- `Theme::DESTRUCTIVE`: shared red feedback for close, chip removal and reset.
- `Theme::SUBTLE_ACCENT_OPACITY`: 0.12, shared by row backgrounds, idle
  scrollbars and progress-bar remainders. It is a gamma-multiplied accent tint,
  not a separate blue. Progress bars use square full-accent fills and keep
  their status text inside, with contrasting ink over the filled portion.
- `SPACING`: 10 logical pixels. Windows, inputs and scroll handles are square;
  row highlight corners still come from their theme.
- `plain_style` installs the global base style. Windows, dropdowns and modals
  apply their own theme's scrollbar visuals locally. Avoid global style changes
  for individual controls.
- Section separation uses space instead of decorative lines. Checkbox split
  dividers and intentional diagnostic rules remain functional separators.
- Physical readouts use the shared `format::number` formatter: comma thousands
  separators and a decimal point (`21,580.174 km`). Existing precision, unit
  selection and scientific notation for trace quantities remain unchanged.
  Formatting is presentation-only; numeric API fields, identifiers and editable
  command/query syntax are not rewritten.
- `Slider` rows split like checkboxes: label, a text-mode `[=====|-----]`
  track and the value, then the square `+` that opens their description.
  Labels share a column so neighbouring tracks line up. Use them instead of
  egui's slider in the debug console.
- `eyebrow_text` supplies the shared selectable section/table-label typography.
- `TooltipExt::on_hover_text_themed` supplies opaque, theme-matched tooltip
  frames and text, independent of translucent window fills.

## Interaction and animation

`kit/interact.rs` owns the 0.18-second cubic ease-out and highlight painter.
Hovering and leaving use the same curve. Rows combine hover, held-press and
selection into one target, so changing between those states does not restart
or jump the highlight. Read-only facts retain feedback during a press too.

Interactive kit controls use click-and-drag sense, but do not move themselves
on a drag. `held_click` preserves ordinary click-on-release behaviour during
long presses and slow frames. Only dedicated window resize/header handles,
range sliders and HUD controls consume movement.

Buttons, search fields, number fields, range components and dropdowns use two
2 px vertical accent edges instead of bottom underlines, matching table rows.
The edges grow from and retract towards their vertical centre with the shared
easing, retaining existing hover, focus, held-press and selection behaviour.
Removable chips share whole-chip accent edges across both hit regions; only
the remove glyph turns red on hover or held press. They have independent
edit/remove hit regions. `kit::Pills` owns their visual lifecycle: callers pass
the current keyed labels every frame, never manually discard rendered chips.
All additions, removals and label replacements animate through the same
retargetable motion, including changes made outside the chips. Outgoing visuals
are non-interactive; re-adding mid-exit reverses without snapping. The raw chip
painter is private so callers cannot accidentally bypass this behaviour.
The Type dropdown shares the same curve for button width,
arrow rotation and menu reveal. `animated_rect` in `motion.rs` retargets layout
motion from its current displayed rectangle, including mid-animation resizing.
Inline filter rows use `expandable` with the same arrow painter and reveal
curve. Their clipped bodies reserve only the animated height and cannot accept
actions while opening or closing; caller-owned drafts survive folding.

Scene halo text is a separate lightweight overlay, not a framed kit widget.
Keep its existing scene-specific appearance and pointer-capture behaviour.

## Windows and scrolling

- `kit::Window` is shared by the browser, Tasks, Object Inspector and debug
  console. `Window::new(key, title)` takes a stable key: egui state, the
  remembered position and the dock choice belong to the key, not the title.
  Sizes are set with the `size`/`min_size` builder methods.
- A floating window moves only from its padded header, excluding the header
  buttons; all edges and corners resize it. Bounds are clamped to the scene
  (the screen beside any sidebar) and the window minimum.
- Clicking a window brings it forward. The existing scene-blur registration
  stays in the shared frame; do not replace it when adding a panel.
- `aligned_close_button` centres the visible glyph on title ink, rather than
  the whole header block or font line box. Modals use the same helper. The
  dock button sits in the next header slot along, aligned the same way.

### Docking

- The dock button moves a window into the right-hand sidebar (`kit::dock`)
  and back. The sidebar is full height, opaque and unblurred: `dock::begin`
  reserves its width at the start of the frame and returns the remaining scene
  rect, which becomes the camera viewport and the HUD's bounds. Nothing of the
  3D view lies behind it, so keep the sidebar on the right: canvas markers
  assume the viewport starts at the window's left edge.
- The first time it opens, the sidebar takes the docking window's floating
  width; afterwards it keeps the width last chosen. Its left edge resizes it
  instantly, in whole points, between the widest docked window's minimum and
  half the screen, with the window edges' pointer-origin highlight.
- Floating window content always fills the size egui keeps for the window, so
  short content (such as the inspector's empty prompt) cannot shrink the frame
  away from its header buttons.
- Several docked windows share the sidebar as tabs; only the front one draws.
  A newly docked or reopened window comes to the front. Closing or floating it
  shows the next tab; the sidebar closes with its last tab.
- Each window remembers being docked while closed, and reopens there. Open
  windows check in every frame, so one closed by a shortcut leaves the strip.
- Shortcuts and icons use `kit::toggle_window`: a window docked behind
  another tab comes forward rather than closing. `reveal_window` opens and
  brings forward, as `search` commands do for the browser.
- Content keeps one egui ID in both modes, so scroll positions, folds and text
  survive docking. `kit::layout::scene` gives later panels, including the debug
  console's separate system, the frame's scene rect.
- Use `kit::ScrollArea`, not raw `egui::ScrollArea`, for application panels.
  It defaults to wheel/scrollbar input with content dragging disabled, so
  scrolling cannot steal a control's held press.
- Scrollbar handles stay full-width and square. Tracks are invisible. Idle
  handles use the subtle accent tint, easing to full accent on hover or drag.
  Show handles only when scrolling is useful; dropdown reveal uses an explicit
  decision based on full content height to keep its geometry stable.
- Scrollbar response IDs depend on the pinned egui 0.33 implementation. Check
  `scroll.rs` when upgrading egui; its named-area regression test protects the
  ID mapping and easing in both directions.
- Restore local styles after scoped work. `ScrollArea` restores its opacity
  settings after rendering; row/fact groups scope their zero vertical spacing.

## Extension checklist

1. Put reusable behaviour in `ui/kit/`, with a focused module for stateful
   controls (`pill`, `dropdown`, `number`, `range`, `scroll`, `dock`). Export through
   `kit.rs`; callers should not depend on private implementation modules.
2. Use the owning theme and shared spacing/easing. Reserve button slots with
   `button_size` so layout and rendered controls agree in both themes.
3. Scope persistent widget IDs to the owning UI. Use `push_id` or
   `make_persistent_id` when controls repeat, including expandable facts.
4. Keep animated children inside a reserved, clipped viewport. An outgoing
   pane/menu can remain for animation, but must not accept stale actions or
   increase window bounds.
5. Use virtualised `show_rows` for large uniform lists. Cache expensive object
   descriptions outside painting loops; do not generate systems every frame.
6. Add egui regressions for geometry, long presses, theme feedback and stable
   IDs. Larger feature test suites live beside their modules in `tests.rs`.
7. Run `./scripts/check.sh`. See [Development](../../development.md#checks) for optional
   graphical checks and [Object browser](browser.md) for its controls.
