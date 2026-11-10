# Settling cells — a finished cell keeps its rows until something takes them

## The problem

The live region is **content-anchored**: its top row is fixed, it grows
downward, and it scrolls the screen only once it would run past the bottom
(`CLAUDE.md` invariant 3). So the box's position on screen is the sum of
everything above it — and a frame that takes more rows out of the streaming
strip than it commits to scrollback pulls the box **up** by the difference.
The next thing the turn shows pushes it back down.

Every live cell is taller than what it commits, by design:

| cell | live, in the strip | committed |
| --- | --- | --- |
| a thinking phase | `● Thinking…` + up to 3 rows of thought | one `Thought for …` line |
| a running command | header + up to 4 tail rows + the clock row | header + up to 3 head rows + the `… +N lines` hint |
| …whose output's first line is followed by a blank | the same six rows | header + 1 row + the hint — the peek is the output's first block (`docs/long-lines.md`) |
| …once it had run three seconds | one more: a `(ctrl+b to run in background)` row | — |
| a collapsed read/search group | header + the `⎿` value it is on | one `Searched for …` line |

So every hand-off between two cells was a jump up and a jump back down, for
as long as the gap between them lasted — and there is always a gap: a
provider ends a thinking phase the moment it starts writing what follows, and
a tool call has no cell until its arguments have streamed and its batch is
announced; a command's result goes out in a request whose answer takes as
long as the model takes. The reported bug:

> thinking -> flicker -> show tool e.g skill tool -> thinking again the
> flicker and sometimes bash tool -> flicker -> bash tool

Recorded one synchronized frame at a time off a pty, a turn of that shape —
think, load a skill, think, run a command, run another, think, answer —
moved the box up four times (3, 3, 1 and 3 rows) on a paced stub provider,
and up to three times (3, 1 and 1 rows, the first for 46 ms) against a live
one.

## What the earlier fixes taught

This hand-off was attacked many times before this design, and each attempt
is a record of something that does **not** read well. In the order they were
tried:

1. **Padding.** Holding the strip at its old height with blank rows keeps the
   box still, and leaves a finished command over three empty rows for as
   long as the next request takes: *"it occupies 3 empty newlines — it should
   be 1 new line between `… +234 lines` and the status line"*.
2. **The settled line over its thought.** A thought's `Thought for …` line
   took its header's row at once and kept the thought's last rows under it
   until the call's cell landed. It read as *a flash of stale thought*: a
   line saying the thought was over, with the thought still hanging under
   it — for as long as the call took to write.
3. **The settled line over empty rows.** The same rows left blank read as
   *newlines reserved for whatever came next*, even for the 100 ms a quick
   call takes. Moved under the status line instead, they made the status
   line step up and back.
4. **A bare stand-in for the call.** Drawing `● Bash` over `⎿ Waiting…` from
   the call's first named piece gave the thought's rows somewhere to go, and
   put a cell on screen that named nothing the model had written.
5. **A timed linger.** Keeping the `● Thinking…` block up, unchanged, for a
   fixed 300 ms or one second after the phase ended made a quick hand-off
   seamless — and bounced on every call that took longer to write than the
   guess. A longer bound with blank rows behind it only delays the bounce:
   between two commands the next cell is a whole request away.
6. All but the first covered **only the thought**. A command settling
   shorter than it ran, and a read group closing, kept jumping.

Four rules fall out of that record, and the design below is those rules:

- **no row is ever held blank**, not for a moment;
- **a settled line is never drawn over what it stands for** — it appears
  alone, or not yet;
- **nothing is timed**: a cell keeps its rows until they are taken, however
  long that is;
- **every live cell** is covered, by one mechanism.

## The design

**A live cell that ends keeps the rows it stood on and gives them up one for
one as whatever comes next needs them. Its committed lines reach scrollback
the frame it is down to them.**

The box never moves up while a turn is in flight, and scrollback gets exactly
the lines it always got, in the order it always got them — only *when* they
are written changes. What a cell looks like while it waits depends on what
its ending means:

- **A thought keeps its block exactly as it was** — the `● Thinking…` header
  still blinking and shimmering, the same rows of thought, `Thinking for Ns`
  still on the status line. Nothing on screen changes when the phase ends.
  The `Thought for …` line appears in the frame the next cell lands, alone.
- **A read/search group keeps its cell exactly as it was** too, for the same
  reason: its `Read 2 files` line is drawn alone.
- **A command shows that it finished**, because that is news: its bullet
  stops in the outcome's colour and its clock stops (`+12 lines (4s)`, no
  wait left to count, no key to hint). Its output is as true as it was, so
  it stays.

```
● Thinking…                        ● Thinking…                        Thought for 2s · 53 tokens (ctrl+o to expand)
  ⎿  The user wants the champion.    ⎿  The user wants the champion.
     Load the skill first.       →      Load the skill first.       →  ● Skill(web-search)
     Then search.                       Then search.                     ⎿  Running…

⣤⣀ Working… (… Thinking for 2s …)  ⣤⣀ Working… (… Thinking for 2s …)  ⣤⣀ Working… (2s · esc to interrupt)
    the phase is open                  it ended; the call is being         the call's cell took the rows:
                                       written — nothing has changed       one frame, and the line is alone
```

A cell that has given a row up **stands at rest**: once something has landed
in its place the phase is visibly over, and a header still blinking beside it
would say otherwise. The one hand-off that leaves a cell in that state is a
long thought handing over to the *reply*, whose first row is one row — three
rows of thought give two back to the reply and its spacer, and one stays
under the resting header until the reply's first line commits:

```
● Thinking…                         ● Thinking…              (at rest)   Thought for 2s · 53 tokens (ctrl+o to expand)
  ⎿  No tool is needed.               ⎿  Keep it short.
     I can answer from what I know. →                                 →  ● The champion is whoever the record says, and
     Keep it short.                 ● The champion is                      that is the whole of
```

Leaving that row out would lift the box a row and drop it back when the
line commits; leaving it blank is rule one. Three pieces, all pure, and a
thin boundary.

### 1. The picture a cell leaves (`ui::settling`)

Taken the moment the cell ends, **before** the call that resolves it (which
replaces the state the rows are read from), and built from the same walk the
live renderer draws from — `reasoning::thought_tail_rows`,
`tool::command_tail`, `tool_group::live_group_lines` — so the rows a finished
cell keeps are the rows the live one stood on by construction:

| picture | head (`app::SettlingHead`) | body |
| --- | --- | --- |
| `thought_picture` | `Thinking { secs }` — the live header, and the phase's length for the status line's clause | the block's rows of thought, dim italic |
| `command_picture` | `Lines` — its header, the bullet at rest in the outcome's colour | the tail window, then the clock row **stopped** |
| `group_picture` | `Running` — its live header at rest; the renderer puts the blink back | the `⎿` rows under it |

`settling_cell(picture, committed)` pairs a picture with the lines the cell
commits. A cell with no picture — a `Read`, a `Skill`, a command that printed
nothing (`⎿ Running…` settles into `⎿ (no output)`, a row for a row), a
delivered message's bubble — or whose committed form is no shorter than its
picture has nothing to give up: it is `Settling::settled`.

`app::Settling` is the data: a head, a body, the lines it commits, the rows
it keeps this frame (`rows`) and the rows its committed form takes
(`settled_rows`). `keep` only ever lowers `rows` — a row given up is a row
something else is standing on. A tail body gives up its **oldest** rows
first, the way the live window dropped them (the row left on top takes the
`⎿` corner, and a command's footer counts the newly hidden rows); a group's
rows go from the bottom.

`settling_lines(cell, pulse)` draws it. While it holds rows of its own: the
head — blinking with the frame clock while the cell `is_whole`, at rest once
it has given a row up — then the body its rows leave. **Down to its committed
height it is what it commits**, line for line: the only place a thought's or
a group's settled line is ever drawn in the strip, and never over the content
it stands for.

### 2. Where it is drawn

The strip's preview slot is the settling cells, oldest on top with one blank
row between them — where each will sit in scrollback — then a blank row, then
whatever is live (`ui::live::preview_lines`, sized by `ui::preview_rows` from
the same counts). The status line keeps a thought's `Thinking for Ns` clause
while its block is whole (`settling::lingering_status`,
`App::lingering_thought`). Nothing else about the region changes: the preview
slot is still the region's one elastic part, still clamped to
`ui::preview_budget`.

### 3. How many rows each cell keeps (`ui::settle_plan`)

Before every inline frame, from the viewport's own bookkeeping
(`SettleGeometry`):

```
need = painted_bottom − (view_top + pending_rows + height − extras)
```

`height − extras` is the region were every cell to commit now (a cell's
committed lines and their spacer take the place of its head and the blank row
under it, so each extra row is exactly a row the region is shorter by);
`pending_rows` push it down; `painted_bottom` is where it was last painted to.
If the region would end above that, the difference is kept — by the **newest**
cells first, so the oldest are the first down to their committed height and
scrollback takes cells in order. A frame in which something else grew by as
much — the next cell landing, a reply's first row, a queued message — needs
nothing, and every cell goes.

`settle_frame` applies the plan and releases the **prefix** of cells that are
down to their committed height: a cell with nothing left to give up still
waits behind an older one that holds rows.

A cell never keeps more than it has. Where the region shrinks for a reason no
cell covers — a toast expiring, a menu closing — the box moves by the rest,
as it always did.

### 4. The boundary (`tui::settle`, `term::Commits`)

- `Session::commit_cell(lines, picture)` is the one commit site for a
  **cell** — a block that stands apart under a blank row of its own. It
  either holds the cell (`InlineViewport::hold` keeps its lines and spacer
  back from the next flush; `App::push_settling` puts its picture in the
  strip) or commits it at once behind the usual reseat.
- `Session::sync_settling` runs before every inline frame: it applies the
  plan and queues the released cells' lines, so the frame that draws whatever
  took their rows writes them with it — one synchronized frame, as every
  commit is (`docs/flicker.md`).
- **Order is kept by construction.** `InlineViewport::insert_before` queues
  whatever is held *first*: anything committed while a cell waits is newer
  than it. So no commit site has to know a line is held — a reply's lines, a
  red notice, an interrupt's cells all push the hold out ahead of themselves.
  (An **empty** commit releases nothing: a reply that has completed no line
  yet commits an empty batch on every chunk.)
- **A cell that lands behind a settling one waits behind it.** A message
  queued into the turn is delivered at the round boundary straight after a
  tool's result, and so is a background notice. Committed at once, the bubble
  would push the command's cell out with its rows not yet taken — the box
  jumped two rows there. Routed through `commit_cell`, it stands in the strip
  under the command, drawn as what it commits, and both go together.
- A rebuild from history **drops** the hold (`reflow` clears it, the callers
  clear the cells): the rebuilt tail carries those cells in their committed
  form. A retry that withdraws a thought its failed attempt produced rebuilds
  the same way. A quit **flushes** the hold (`restore`): a held cell is
  committed content.

### When a cell is not held

`ui::settling_holds` — the conversation is the inline view on screen, a turn
is in flight in it, no modal prompt has replaced the region, and the strip
fits its budget — and at the boundary, no framed view's page is flowed into
scrollback. Otherwise a cell commits at once, exactly as before:

- **Outside a turn** nothing follows to take a cell's rows. The turn's end
  releases whatever is still held, ahead of its summary.
- **Under a prompt** the region is the prompt's, not the strip's.
- **Under an alternate-screen overlay** nothing inline is painted; commits
  queue, as always (invariant 4).
- **On a terminal too short for the strip** its overflow flows into
  scrollback instead (`docs/strip-flow.md`); a held cell there would be rows
  the region does not have.

An agent session view holds its own agent's cells the same way — the
pictures read the viewed agent's queue and thought — and entering or leaving
one rebuilds the screen, which drops whatever the other conversation held.

## The clock row carries the Ctrl+B hint

A running command's hint used to be a row of its own that appeared three
seconds in (`TOOL_BACKGROUND_HINT_DELAY`): the cell grew a row at second
three and lost it when the command ended, so its height depended on the
clock. It closes the clock clause now —

```
     +18 lines (22s · wait 2m · ctrl+b to run in background)
  ⎿  Running… (4s · ctrl+b to run in background)
```

— in full where the row has room, in its shorter wording
(`ctrl+b to background`, `ctrl+b to stop`) where it does not, and not at all
on a row too narrow for either: the clock is what the row is for
(`tool::clock_row`). The agent tree's hint joins its header's
`(ctrl+o to expand · …)` the same way, and the collapsed read/search group's
its clock clause. A lone live agent keeps the hint on a row of its own: its
committed cell stands on three rows too, so that row is never one the cell
gives up.

## What it does not change

- **Scrollback.** The same lines, in the same order. Verified by diffing the
  final transcript of the fixed binary against the previous one over the same
  turns — every offline demo, and the real backend against a stub with the
  box pinned and floating, 100×30 down to 80×10, with a Ctrl+O round trip,
  an Esc, a resize, a permission prompt, an open `/settings` menu and a
  message delivered mid-turn, each while a cell was settling: identical.
- **History, the rollout, the derived context.** A cell is recorded the
  moment it ends, as before; only its lines' flush waits.
- **Ctrl+O and Ctrl+D.** Built from history, so a settling cell is already
  there in its committed form.
- **The thought's text never commits.** It is the strip's while the block
  stands and gone when the block is (`docs/thinking-stream.md`).
- **Region growth, the preview budget, the strip flow, modal geometry.**

## Testing

- `app::tests::settling` — the cell: full and committed heights, `keep`'s
  clamps, what each body shows at each height, when it is whole, what
  `lingering_thought` reports.
- `ui::tests::settling` — the pictures agree with the live renderers row for
  row, **style for style and at several points of the blink**; the settled
  line is never drawn over its thought at any height; a cell that gave a row
  up stands at rest; a cell at its committed height is its committed lines;
  the plan's arithmetic (which cell keeps rows first, the prefix release,
  each condition under which nothing holds).
- `ui::tests::settling_frames` — whole turns played frame by frame through a
  model of the boundary over the real pure core, checking on **every frame**
  that the box never moved up, that the strip never shows two blank rows in a
  row or ends on one, that no `Thought for …` line sits over a row of
  thought, and at the end that scrollback is exactly what a rebuild from
  history paints. Flush with the bottom and floating; thought → call, thought
  → reply, command → command, a peek cut at a blank line, a parallel batch,
  a read group closing, a delivered message, a terminal too short to hold a
  cell, a turn ending on a settling cell.
- `term::tests` — `Commits`: held lines go ahead of whatever commits next, an
  empty commit releases nothing, releases are oldest-first, a rebuild drops
  both.
- Every guard above was mutation-checked: removed or inverted, a test fails.
- `scripts/smoke.sh` Phase 153 samples whole turns in tmux — the dummy's at
  two heights, and the real backend against a paced stub provider — for a
  footer that moved up, a status line under more than one blank row, or a
  settled thought line over its thought, and checks every cell reached
  scrollback exactly once, in order. It fails on the build before this.
  Phase 66 keeps the rule that a thought's text is off the screen the moment
  its line shows and never reaches scrollback; Phase 42 that the Ctrl+B hint
  never adds a row.
