# Object browser

The object browser opens with the search icon, Ctrl+K, or a `search` command.
It builds the shared search query using registry-driven filter chips, drawn
with the [shared UI kit](kit.md). The query and engine are described in
[object search](../../simulation/search.md).

## Search and filter controls

- The search box comes first. Below it, the controls run **Within**, **Show**,
  **This system** (when available), then **+ Add filter**, with **Refresh**
  and **Reset** right-aligned. **Reset** sits to the right of **Refresh** and
  appears only when the query or search-box text differs from the defaults.
  Controls wrap when needed on narrow windows.
- **Type** and filter chips always start on a separate row below those controls.
- Reset retains its full behaviour: clears designation/search text, Type,
  property filters and relationships, and restores Within to 50 ly and Show
  to 50. Its opaque tooltip explains this. Its animated end edges are destructive red.
- Type lists available result kinds from the search registry. **All types**
  removes explicit kind/family restrictions, but other filters can still
  narrow the kinds that match. Selecting one kind removes incompatible
  property filters and relationships.
- Queries from the CLI can contain a family or several kinds. Type displays
  that combination rather than pretending a single menu option is selected.
- A chip's text opens its editor. Its separate `×` action removes the filter
  immediately, retaining a non-interactive visual until its exit animation
  completes. The shared chip collection reconciles the query each frame, so
  Reset, incompatible Type changes, search text, incoming search/clear commands
  and edited chip values all get the same entrance and exit animation.
  Re-adding a chip mid-exit reverses from its current width without a snap.
  Hovering either half lights two theme
  accent edges at the outside ends of the whole chip, with no inner edges.
  The remove glyph alone eases to the same red as window close when hovered
  or pressed; its hit area has no separate red edges.
- Designation or ID text keeps the existing filters. Full `ns search` syntax
  replaces the query. Search-box text commits on Enter; invalid text is
  explained without replacing the last valid query.
- Query changes submit `UiCommand::Search` once per distinct query. Search
  execution stays outside the UI, using the native service or browser worker.
- **Refresh** remains beside the scope/count controls. It resubmits the query
  from the camera's current position, because existing result distances are
  measured from the position where the search began.
- **This system** is offered when the current location resolves to a searchable
  generated system. Editing Within returns to radius-based scope.
- There is no command-copy control or browser footer.

## Number fields

- `Within: [50] ly` and `Show: [50]` are the defaults.
- Both use `kit::number_field`, with the label, editable number and optional
  unit inside one shared frame.
- Horizontal padding and gaps between the label, number and unit are 10 px.
  The field grows and shrinks with the number's text, including while editing.
- Edits apply on Enter or focus loss, not on every keystroke.
- Clicking the label, unit or padding focuses the number and selects all
  its text for replacement. Clicking the digits keeps normal caret placement
  and text selection.
- Result counts are whole numbers from 1 to 1,000. Distances allow decimals,
  must be positive and are capped at 10,000 ly, following the query's bounds.
- Out-of-range counts and distances above the maximum are clamped. Invalid
  text, fractional counts and nonpositive distances revert to the last value.
- Reset and incoming search commands update the displayed values.

## Filter picker and numeric ranges

- Add filter opens a theme-matched modal. The find box narrows
  property and relationship choices by label or ID. Result kinds live in
  Type instead; relationship target types remain in this picker.
- Choices come from the registry and the query's remaining compatible kinds.
  Already-used properties are not offered as new filters; edit their chips.
- Menu items have no vertical gaps. Group headings use breathing room rather
  than divider lines, and there is no helper footer.
- Each item expands its controls directly beneath its row, keeping the picker
  list visible. Reveal and collapse use the shared 0.18-second cubic ease-out;
  the thin down arrow rotates upwards when open and back down when closed.
  Open rows retain the table's selected-row highlight even away from the pointer;
  clicking closed returns them to ordinary hover feedback.
  Draft values survive folding a row. Expanded controls have only **Add filter**,
  with no Back button or second window. Existing chips still open their editor.
- The list can grow to 560 logical pixels high, capped against viewport height
  with space for the header, search field and padding. Short lists shrink.
  The modal resets its available height each frame so the list is not limited
  by an inherited modal size or a previously shorter editor/search result.
- Every numeric property uses the shared **Min / Max** field, unit and
  two-handle slider. Blank endpoints mean unbounded. At least one endpoint is
  needed to add a filter; the minimum cannot exceed the maximum.
- Numeric text accepts scientific notation and comma, underscore or space
  separators. Nonfinite numbers and fractional integer counts are rejected.
- Sliders use property-specific linear or logarithmic exploration spans.
  These are not validation limits: typed values can extend the span. Zero or
  negative endpoints switch logarithmic exploration to linear rather than
  silently clamping typed values.
- Counts use whole steps. Temperature uses 1 K steps and eccentricity uses
  0.001 steps. Other slider-generated values use five significant digits,
  with scientific notation for very small or large values. Manually typed
  precision is not reformatted by the slider until that endpoint is moved.
- Logarithmic sliders show `Log scale · N decades` (or `1 decade`) under the
  track, not in a tooltip. Linear sliders show `Linear scale`.
- Slider keyboard control: Up selects the maximum, Down the minimum; Left
  and Right move the selected endpoint, with Shift taking larger steps.
- Choice properties allow any of the selected values; flags use Yes/No.
  **Add filter** or **Update filter** applies the editor. Invalid editors
  cannot be submitted.

## Type dropdown

- The button width, small thin arrow rotation and menu reveal/close all use
  the shared 0.18-second cubic ease-out.
- The animated end edges are hover-only, not a persistent open-state indicator.
- Menus are opaque and edge-to-edge, without frame padding or inter-item gaps.
  The selected item uses the same row highlight as the result table.
- The full content geometry is kept stable during reveal, including the
  scrollbar decision and rounded-up menu width, to avoid clipping its right
  selection edge.
- Picking an option, clicking outside, pressing Escape or moving the button
  (including moving its parent window) closes the menu. Exiting menu content
  is disabled while the closing animation finishes.

## Results table

- Running searches show their status text inside a square progress bar. The
  completed portion uses the full theme accent; the remainder uses the same
  12%-strength accent as idle scrollbars. Text switches to dark ink over the
  fill for contrast. Searches without a known fraction show a moving band.

- Result clicks change application selection without travelling. Highlighting
  follows `Navigation::selection`, including selection and deselection from
  the scene or another UI. Travel remains an explicit inspector action.
- OBJECT · SYSTEM, CLASS and DISTANCE are real text labels, using the same
  selectable eyebrow typography and top alignment as SELECTED.
- The scrollbar appears only when results overflow the viewport. Rows retain
  space for the gutter even without a scrollbar, keeping headings aligned.
  Square handles stay full-width, with no visible track. Their colour is the
  theme accent at 12% strength normally, easing to full accent on hover or
  drag and back on leaving. Hover never changes their width.
- Rows meet without inter-row gaps. Former external spacing is included
  inside each row's hit area and highlight, with the text centred vertically.
- Hovered and selected rows use the same kit highlight painter, background
  colour and two bright outer edges. Selection keeps the highlight fully
  active; hover animates into and out of that same appearance.
- Rows are virtualised through `show_rows`; only the visible slice builds
  widgets. Long captions are truncated instead of growing the window.
- A selected result slides its facts and actions in beside the table when the
  result viewport is at least 720 logical pixels wide, or below it otherwise.
  The table and readout share the same retargetable easing during resizing.
- The outgoing readout stays briefly for its exit animation, with actions
  disabled. Off-screen animated children are clipped and never increase the
  parent window's size.
- Selected-object facts are contiguous and reuse the shared fact rows;
  descriptions and actions come from the existing inspector.

## Code locations

Paths below are relative to `crates/client/src/`:

- `ui/browser.rs`: browser state, query controls and command wiring.
- `ui/browser/filters.rs` and `picker.rs`: filter chips and their editor.
- `ui/browser/results.rs`: status, results table and selected-object details.
- `ui/browser/tests.rs`: query, selection, responsiveness and rendering regressions.
- `ui/kit/number.rs`: shared labelled numeric field.
- `ui/kit/range.rs`: range text, sliders, mapping and precision.
- `ui/kit/pill.rs`: removable animated filter chips.
- `ui/kit/dropdown.rs`: animated Type dropdown and shared arrow painter.
- `ui/kit/expandable.rs`: inline animated picker rows and clipped control reveal.
- `ui/kit/widgets.rs` and `interact.rs`: controls, rows and shared feedback.
- `ui/kit/motion.rs`: retargetable layout animation.
- `ui/kit/scroll.rs`: native scrolling with animated accent opacity.
- `ui/kit/theme.rs`: colours, typography and theme values.

## Extending the browser

1. Add kinds, properties or relationships to the search registry and engine,
   not to a separate UI list. Compatibility and labels flow into the browser.
2. For a numeric property, add its exploration span in `picker::slider_span`.
   The regression test requires valid slider configurations for every numeric
   registry property. Choose linear/logarithmic mapping and step deliberately.
3. Keep edits in `filters.rs` and the shared `Query`; send actions as
   `UiCommand`s. Do not create a second selection or travel state in the panel.
4. Reuse kit controls and animations rather than adding local copies. Test
   narrow layouts, selection changes, unfinished numeric text and named
   scroll areas before updating the web build.
