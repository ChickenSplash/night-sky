# Object browser

The object browser opens with the search icon, Ctrl+K, or a `search` command.
It builds the shared search query using registry-driven filter chips.

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
  must be positive and are capped at 1,000 ly, following the query's bounds.
- Out-of-range counts and distances above the maximum are clamped. Invalid
  text, fractional counts and nonpositive distances revert to the last value.
- Reset and incoming search commands update the displayed values.

## Results table

- The scrollbar has a dedicated gutter wide enough for its expanded hover
  state, including its margins. It stays thin normally and expands on hover,
  without covering row text or highlights. The gutter stays allocated so
  column headings remain aligned and hovering does not shift the table.
- Rows meet without inter-row gaps. Former external spacing is included
  inside each row's hit area and highlight, with the text centred vertically.
- Hovered and selected rows use the same kit highlight painter, background
  colour and two bright outer edges. Selection keeps the highlight fully
  active; hover animates into and out of that same appearance.

## Code locations

Paths below are relative to `crates/client/src/`:

- `ui/browser/mod.rs`: browser state, query controls and command wiring.
- `ui/browser/filters.rs` and `picker.rs`: filter chips and their editor.
- `ui/browser/results.rs`: status, results table and selected-object details.
- `ui/kit/number.rs`: shared labelled numeric field.
- `ui/kit/widgets.rs` and `interact.rs`: list rows and shared highlight painter.
- `ui/kit/theme.rs`: colours, typography and theme values.
