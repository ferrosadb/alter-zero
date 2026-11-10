# Live cells settle in place — a live cell is never taller than what it leaves behind

## The report

> thinking → flicker → the tool shows (a skill call) → thinking again, the
> flicker → and sometimes bash → flicker → bash. Maybe it is the rows hiding
> or adjusting: thinking shows three lines and then hides them, and the bash
> tool goes from four lines to three, and cuts where there is an empty line.

Both guesses were right. Nothing was torn and nothing was redrawn twice — what
moved was the **input box**, up and then back down, at every hand-off between
one live cell and the next.

## What the frames showed

Each run was recorded with `tmux pipe-pane` (every chunk timestamped) and
replayed through a terminal emulator one synchronized frame at a time — a
frame being everything up to an `ESC[?2026l`, exactly what a mode-2026
terminal presents at once. The row the session footer sits on, frame by
frame, is the box's position.

The reported prompt — *"Who is the current world chess champion? use skill
tool first dont produce intermediate response"* — against OpenRouter's
`recursive/flint-v1.0:free` at `medium` thinking, 100×30:

| frame | time | footer row | what the frame shows |
| --- | --- | --- | --- |
| #39 | 1 630 ms | 17 | `● Thinking…` over three rows of thought |
| #73 | 2 374 ms | **14** | `Thought for 0s · 172 tokens` committed; the strip is the status line alone |
| #76 | 2 435 ms | 17 | `● Skill(web-search)` lands; the box back down |
| #112 | 3 519 ms | 23 | the next thought, three rows again |
| #179 | 4 999 ms | **20** | it settles, and the model starts writing a `bash` call |
| #195 | 5 327 ms | 24 | `● Bash(python3 …)` lands, a third of a second later |
| #321 | 9 481 ms | 30 | the third thought |
| #369 | 10 533 ms | **27** | it settles |
| #392 | 10 918 ms | 30 | the next `● Bash(…)` lands |
| #499 | 14 141 ms | **29** | the fourth thought gives way to the reply's first words |
| #503 | 14 262 ms | 30 | the reply's first line commits |

Four jumps in one turn. A second prompt that runs two commands showed the
other half — the box rising **one** row as the first command's tail settled
into its peek, and **two** as the second's did, because its output opened on
a title and a blank line.

Driven offline, where every hand-off can be staged (the dummy backend, and
the real backend against a paced stub provider):

| hand-off | rows the box rose | until |
| --- | --- | --- |
| a thought → the round's tool call | up to 3 | the call's cell landed, once its arguments had streamed |
| a thought → the reply's first row | 1 | the reply's first line committed |
| a thought → a task call, which has no cell | up to 3 | the next round's first content |
| a running command's tail → its settled peek | 1 | the next round's first content |
| …three seconds in, with its Ctrl+B hint row up | 2 | the same |
| …when a blank line closes the settled peek early | up to 3 more | the same |
| the collapsed read group → the next call of its batch | 1 | that call's output grew |
| any of the above inside a **subagent's session view** | the same | the same |

## Why

The live region is **content-anchored** (CLAUDE.md invariant 3): a frame that
commits `c` rows to scrollback pushes it down by `c`, and a frame whose region
is `h` rows shorter pulls its bottom up by `h`. So the box rises exactly when
the strip gives up more rows than went to scrollback in the same frame — when
a live cell **stood taller than the rows it committed**:

- **The thinking block** stood on up to five rows — the `● Thinking…` header,
  three rows of thought and the strip's gap — and settled into two: the
  `Thought for …` line and its spacer. The backend closes the phase at the
  round's *first tool-call piece*, but the call has a cell only when its batch
  is announced, after its arguments have streamed; in between the strip had
  nothing to show. That interval is why it happened "sometimes": a call whose
  pieces, the stream's end and the batch all land inside one frame never shows
  the hole, and a `bash` command a second long to write shows it for a second.
- **A running command** tailed four rows over its `+N lines (Ns · wait …)`
  clock row — and, three seconds in, a `(ctrl+b to run in background)` hint
  row under that — and settled into three rows over the
  `… +N lines (ctrl+o to expand)` hint. The settled peek is also the output's
  **first block** (`docs/long-lines.md`): a title, a blank line and forty rows
  settle into `title` over the hint, two rows where the tail stood on five.
- **The collapsed read group** stood a header over a `⎿` hint row and settled
  into one line. Alone in the strip that is covered by whatever ends it; as
  the first call of a batch whose other cells are already up, nothing covers
  it.

Every one of these is followed by more content, so the box came back down —
the next cell, the next round's thought. Up and then down is what read as
flicker.

## The two fixes before this one

1. **A preview floor.** Each frame the strip reserved what it had a frame ago
   less every row committed since, padded with blank rows — `StreamRender`'s
   own rule inside a streaming reply (`docs/slow-stream.md`, which stands:
   there the next row is a token away), lifted to every hand-off between the
   strip's cells. The box stopped moving, and the padding showed wherever the
   next content was more than a moment away: *"it occupies 3 empty newlines —
   it should be 1 new line between `… +234 lines` and the status line"*. A
   command's settled peek sat over three blank rows until the next request
   answered.
2. **A held thought.** The settled thought kept its rows — its `Thought for …`
   line with the thought still under it — until the call's cell landed, the
   line held back from scrollback meanwhile. That needed a hold in the
   viewport, in `App`, at every commit site and in every rebuild, and it
   covered one hand-off of the table above: a thought before the reply, a
   thought before a task call, every command and the read group still moved
   the box.

Both tried to hide a shrink after it had happened: with blank rows, then with
rows that should have gone. The shrink is the bug.

## The rule

> **A live cell is never taller than the rows it leaves behind.**

When that holds, a cell settling commits at least the rows the strip gives
up, and the box cannot rise — whatever follows the cell, and however long it
takes to arrive. There is nothing to hold and nothing to pad, so there is no
state: no floor to track, no line held back from scrollback, no rule about
what counts as "the next cell". The fix is in what the live cells look like.

**Nothing that reaches scrollback changes.** Every committed cell, the Ctrl+O
transcript, the rollout and the derived context are byte for byte what they
were; only the transient strip is reshaped, to stand on the rows it is going
to keep.

## Each cell

### A thought is one row

```
● Thinking… the structure should be: a hero, three feature cards, a footer.
```

The label — blinking bullet, shimmering `Thinking…` — and beside it the row
of thought still being written, dim and italic (`ui::live_reasoning_lines`).
It settles into the one `Thought for 3s · 228 tokens (ctrl+o to expand)` line:
the strip trades the row for the line and its gap for the line's spacer, and
the box does not move.

One row is the only height that works. A block of `n` rows settles into the
line and its spacer, so it needs `n - 1` rows of whatever comes next landing
in the same frame: a tool's cell brings two, the reply's first row one, a
task call none. Only `n = 1` needs nothing — which also makes the timing
irrelevant, so there is nothing to coordinate with the call still being
written.

The row shows the thought's **last wrapped row**, not its last columns: it
starts at a word and grows to the right until it wraps, so it reads as text
arriving, where a window sliding over the line's tail would move every glyph
on every token. Blank lines are skipped, and a terminal with fewer than
`REASONING_TAIL_MIN_COLS` (8) columns beside the label keeps the label alone.
The whole thought is in the Ctrl+O transcript while it streams and after
(`docs/thinking-stream.md`), as it always was.

### A running command stands on the rows it settles into

`ui::tool::running_command_lines` sizes the tail by
`settled_command_rows`: **the fewest rows the cell can settle into, whatever
the command goes on to print**.

| the output so far | the running cell | settles into |
| --- | --- | --- |
| nothing | `⎿ Running… (10s · wait 2m)` | `⎿ (no output)` |
| a few rows, one block | every row, the clock on the end of the last | the same rows |
| long | **three** tail rows over `+N lines (22s · wait 1m 38s)` | three head rows over `… +N lines (ctrl+o to expand)` |
| `title`, a blank line, more | **one** tail row over the clock row | `title` over the hint |

The settled height is not simply "what `tool_lines` would build now". The
fold can be *shorter* than the short output it replaces: `title`, a blank
line and one more row show whole, three rows, and become `title` over the
hint — two — once the output runs past the fold. A tail sized to the cell of
the moment would have shrunk mid-command. So the height is the smaller of two
numbers, each of which only grows as the command prints: every row printed so
far, and the fold the output would settle into if it went on —
`min(first block's rows, TOOL_FOLD_ROWS) + 1`. The tail never shrinks while
the command runs and never stands taller than the cell it becomes
(`a_running_cell_only_grows_and_never_past_the_cell_it_becomes` walks every
prefix of several outputs).

The settled cell also **reshapes** a line that is a JSON document
(`docs/long-lines.md` §6), so the rows are counted on the reshaped lines —
the ones the cell will show — while the tail goes on showing the output as it
was printed. That almost always makes the settled cell taller. The one way it
makes it shorter is an empty document padded past the width (`[`, two hundred
spaces, `]` settles as `[]`): a cell sure of a single row has no room for a
window, so its clock row stands alone — `⎿ +2 lines (9s · wait 2m)`.

The measurement runs on every frame a command is on screen, several times a
frame, so it is cheap by construction. The fold caps the first block at
`TOOL_FOLD_ROWS` and the whole at one row more, so the walk stops at the
first line past which neither number can change the answer — a long build
log is measured from its first few lines, and at most that many are ever
parsed as JSON. The cell's display rows are counted once per build, where the
total, the window's place and the window's rows each used to count them
again — and that count is nearly the whole cost of a build over a long tail
(2.2 ms of the 2.3 a 64 KB tail takes in a debug build).

The window still ends at the lowest row **still moving**
(`docs/tool-streaming.md` *The window follows what is still moving*); only
its size changed, from a constant of its own (`TOOL_PEEK_ROWS`, retired) to
the fold's.

**The clock has no row of its own where the settled cell has none.** For an
output that fits, the clock rides the end of the last row —
`beta (9s · wait 2m)`, the read group's own rule for its command row — in the
clock's dim beside the output's ink. Output is never cut to seat it: a last
row with no room shows whole and the clock sits that frame out, since what a
command just printed ends at the right edge — a progress bar's rate and its
ETA.

**The Ctrl+B hint joins the clock's row** — `+34 lines (22s · wait 1m 38s)
(ctrl+b to run in background)` — where the two fit, and is left out whole
where they do not (`ui::tool::with_hint`). The key works either way; a hint
that wrapped would cost the row it was folded in to save.

### The read group gives up its hint row beside other cells

The group's live cell wears its `⎿` hint row only while it is the strip's
**only** cell (`live_group_lines`' `alone`). Between rounds, or as a batch of
reads, what ends it lands on its rows. Beside an agent tree, the rest of a
mixed batch or a `write` streaming under it, it is its header alone — the
height of the line it becomes — and the other cells say what is running.

Three smaller things in the same cell:

- its command row's clock (`$ grep -rn todo . (12s · 34 lines)`) and the
  Ctrl+B hint after it ride the row or are left out; a clock on a row of its
  own was a row the cell grew two seconds into a search and gave back when
  the search ended, the group still open;
- a `write` the model has started but not yet named a path for gets no cell
  under an open group. In the scratchpad it joins the group, anywhere else it
  ends it — and a `● Write` cell put up before the path said which stood on
  two rows the group took back (`ui::live::drafting_lines`).

### The agent tree and the `!` shell

Their Ctrl+B hint was a row of its own too. It rides the tree's header after
`(ctrl+o to expand)`, a lone agent's `● Agent(…)` header, and the shell's
`⎿ Running… (5s)` row.

## What it does not change

- **Cells that settle taller.** An `edit` settles from two rows into its
  diff, a command's last burst of output may lengthen its peek, a short
  output with a blank line in it shows whole. The box moves *down* for
  those, which is how content arrives.
- **A command that takes back what it printed.** A program that draws three
  rows of progress and erases them on its way out settles into less than it
  showed. The cell stands on what the output so far would settle into; it
  cannot know the output will shrink.
- **What the user asks for.** Ctrl+B moves a running command to the
  background and its cell becomes two rows; Esc ends the turn.
- **The turn's end with a spinner tip up.** The tip hangs off the status line
  and has no committed counterpart, so the idle box settles one row higher.
  It is the end of the turn, not a hand-off between cells.
- **A toast expiring mid-turn**, and the lost-connection row going when the
  network returns: one row, one way.
- **The group's hint stepping from a wrapped value to a short one.** A path
  too long for its row wraps rather than being cut (`docs/tool-groups.md`),
  so the row count can differ between two values of the hint.

## Testing

- **`ui::tests::live_cells`** — the rule itself, cell by cell: the thought's
  block against its settled line at every width; the running command against
  `tool_lines` over every output shape the fold treats differently, and over
  every prefix of several outputs; the clock's and the hint's placement; the
  group with and without company. Then whole hand-offs, folded event by event
  the way `tui::stream` folds them (`Stage`): a thought into its call, into
  the reply, into a task call; a reply into a thought; a command of each
  output shape with the hint falling due on the way; a parallel batch; the
  read group closing under its batch and under whatever ends it; a write
  streaming under an open group; a subagent's session view. Each step asserts
  that the rows committed cover the rows the region gave up. Putting any of
  the old shapes back fails it — a second thought row, one more tail row, the
  hint on its own row, the group's hint row beside other cells, a pathless
  write's cell.
- **`scripts/smoke.sh` Phase 153** — five turns sampled ~20 times a second
  for as long as the status line is up: the footer never moves up (confirmed
  against a second capture, Phase 121's rule), never more than one blank row
  above the status line (what the floor looked like), and never a row under a
  live `● Thinking…`. The default offline turn with Verbose output off, on a
  roomy and on a short terminal; the parallel batch; and the real backend
  against a paced stub provider — one turn through every hand-off in the
  report, then a command slow enough for the Ctrl+B hint to fall due. The
  pre-fix binary fails all five (2, 2, 3, 6 and 1 hops).
- **A real provider** — the two prompts above on
  `recursive/flint-v1.0:free` at `medium`, recorded and replayed frame by
  frame before and after: four jumps a turn before, none after, and exactly
  one blank row above the status line in every mid-turn frame.
