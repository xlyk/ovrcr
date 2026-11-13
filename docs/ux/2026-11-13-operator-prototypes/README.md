# Operator sidebar prototypes

Throwaway HTML prototypes for the Operator section. Four structures share one
39-column sidebar, the same five states, and a sidebar with no Usage section.

![Current app without Usage](current-app-baseline.png)

Open [index.html](index.html) in a browser. Switch with `←` / `→`, the bottom
bar, or the links below:

- `?variant=A` — **Command card.** Rail, glyph, identity, state and hint on a
  tinted three-row card. Strongest identity; closest to the current direction.
- `?variant=B` — **Status strip.** State leads, identity follows; two quiet rows
  with a hairline. Best if the Operator should stop competing with project
  headings.
- `?variant=C` — **Command tile.** A bordered two-row tile with an `o` keycap.
  Strongest affordance; best if opening Operator is the main job.
- `?variant=D` — **One line.** A single prompt-style row. Maximum density; the
  tree gets two extra rows, at the cost of state detail.

Add `&state=off|idle|working|unread|setup` and `&selected=1` to inspect states.
The top buttons set them too.

## Tradeoffs

| Variant | Rows | Identity | Affordance | Density |
| --- | --- | --- | --- | --- |
| A Command card | 3 | Strongest | Hint row | Lowest |
| B Status strip | 2 | Quiet | Hint row | Medium |
| C Command tile | 2 | Medium | `o` keycap | Medium |
| D One line | 1 | Light | None | Highest |

The unread state drops the right-aligned age in every variant so
`Reply ready · 1 unread` stays on one line at 39 columns.

This is a design artifact; the winning structure still needs a Rust TUI
implementation and tests.
