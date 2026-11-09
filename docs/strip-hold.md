# The strip hold — no box hop between one live cell and the next

## The report

> When the model thinks very fast [the three-row thinking cell shows] and it
> calls a tool, e.g. the skill tool, it jumps and creates a flickering effect.
> Not just after thinking: it's using a tool and another tool call — sometimes
> the next tool call produces flickering.

## What the frames showed

Recorded with `tmux pipe-pane` (every chunk timestamped) and replayed through a
terminal emulator one synchronized frame at a time — each frame is everything
up to an `ESC[?2026l`, exactly what a mode-2026 terminal presents at once.
Nothing tore and nothing purge-rebuilt; what moved was the box. Every upward
move of the footer while a turn ran, on `recursive/flint-v1.0:free` at
`medium` through OpenRouter and on a paced stub provider driving the real
backend:

| hand-off | thinking shown for | box rose | back down after |
| --- | --- | --- | --- |
| a thought → `● Skill(web-search)` | ~400 ms | 3 rows | 53 ms |
| a thought → the round's `bash` call | ~500 ms | 3 rows | 287 ms |
| a thought → three `Read` calls | 60 ms – 2 s | 2–3 rows | 52–156 ms |
| a thought → the reply's first words | ~250 ms | 1 row | 102 ms |
| a command's live tail → its settled peek, then the next round | — | 1 row | 209–784 ms |

Up and straight back down a fraction of a second later: the flicker. Most of
the thoughts were under half a second — a `● Thinking…` block with three rows
of text that nobody could read before it collapsed.

## Why

The live region is **content-anchored** (CLAUDE.md invariant 3): a frame that
commits `c` rows to scrollback pushes it down by `c`, and a frame whose region
is `h` rows shorter pulls its bottom up by `h`. So the box rises exactly when
the strip sheds more rows than went to scrollback in the same frame, and every
hand-off between two live cells did that:

- **A thought settling.** The block stands on five rows (the `● Thinking…`
  header, three rows of thought, the gap); its settle commits two (the
  `Thought for …` line and a spacer). The backend settles it at the round's
  first tool-call piece, but the call gets a cell only when its batch is
  announced — after its arguments have streamed and the stream has ended —
  so for as long as the call took to generate, the box sat three rows up.
- **A command's tail settling.** A running `bash` cell carries a clock row
  (`+N lines (2s · wait 2m)`) its settled peek does not, so the round's last
  command lifted the box a row until the next round's first content arrived.
- **The same in a subagent's session view**, whose strip works the same way.

## What was tried before, and why it went

Two earlier fixes (`fix/thinking-handoff-flicker`) were merged and reverted:

1. **A preview floor** — the strip kept what its slot held a frame ago less
   what committed since, padding with blank rows, `StreamRender`'s own rule
   inside a reply lifted to the whole slot. No hop, but the padding never let
   go until content took it back, so between rounds — a command's tall tail
   settled, the next request seconds away — it stood as extra empty lines over
   the status line for as long as the request took ("it should be 1 new line
   between `… +234 lines` and the status line").
2. **A held thought** — the settled `Thought for …` line kept the block's rows
   with the thought still under it until the next cell landed. Content instead
   of blanks, but only for the thought: every other hand-off still hopped, and
   tying a held scrollback line to its round took special cases (a task call
   with no cell, a retried request, an agent view).

## The design

Two rules, one per cause.

### 1. A quick thought puts no block up

A thinking phase previews its live block only once it has run
`REASONING_SHOW_DELAY` (one second). Before that the strip shows nothing for
it: the status line already says `Thinking for 0s` and its token count ticks,
and a block that collapses before anyone could read it is noise. The
`Thought for …` line commits at the phase's end exactly as before, and Ctrl+O
shows an open phase at once — the delay is the strip's alone.

`ui::reasoning::reasoning_block_shown` is the one gate: the main strip reads
the phase's elapsed off the status line (`TurnStatus::thinking`, the
boundary's thinking clock), a subagent's session view off its run
(`AgentRun::thinking`), and `ui::preview_rows`, `ui::live::preview_lines`, the
agent view's twins and the strip flow's key all ask it. The delay is the
spinner tip's rule (`docs/tips.md`) and the Ctrl+B hint's
(`docs/background.md`): what would only flash is not shown.

Most thoughts in the recordings were well under the delay, so most hand-offs
now have nothing to collapse at all: the thought's line commits (the box
moves *down* two rows, like any commit) and the call's cell lands under it.

### 2. The strip holds what it gives back, briefly

When a frame would shrink the strip's upper part — its preview slot (the live
cells and their gap) — by more rows than it commits, the difference stays on
screen as **blank rows directly above the status line**, so the status line,
the box and the footer stay where they were and the next cell lands in the
held rows. The hold ends three ways:

- **Content takes the rows back** — the call's cell, the reply's first words,
  a new block. The usual case: within a round the next cell is tens to
  hundreds of milliseconds away.
- **Commits cover them** — every row committed lowers what is owed, so a
  reply's first line, or a thought's own `Thought for …` line, uses the hold
  up.
- **It lapses** after `STRIP_HOLD` (1.5 s) with nothing having taken it — so
  a hold never lingers through a slow request the way the floor did. The box
  then settles once, which is a layout change, not a flicker.

While a **thinking phase is open** the lapse waits: a phase shows nothing for
its first second (rule 1), and what it settles into is two committed rows that
use the hold up — letting the hold lapse during the hidden second and the
thought's line push the box straight back down was the bounce this exists to
remove.

The numbers came from the recordings: within a round the hand-offs took 52 to
287 ms, and between rounds this model answered in 0.65 to 1.15 s, so a hold
shorter than that would lapse just before the next round's content and put the
bounce back with a longer period. `STRIP_HOLD` and `REASONING_SHOW_DELAY` are
`ui::theme` constants.

### The mechanism

`ui::StripHold` (`ui::layout`) is a pure tracker the boundary owns beside its
`StreamRender`s, run once per painted conversation frame by
`Session::draw_conversation`:

1. `App::set_strip_hold(0)`, so the frame's content is measured on its own.
2. `StripHold::rows(app, committed, width, height, now)` — the upper part the
   last frame painted, less the `committed` rows queued for scrollback since
   (`InlineViewport::pending_rows`), less what the content needs now: the rows
   to hold, clamped to the room the composer path leaves (so a hold can never
   squeeze the box), 0 once it lapses. `now` is the loop's clock, injected —
   time never reaches the pure core except already measured.
3. `App::set_strip_hold(rows)`; the frame paints; `StripHold::painted` records
   the upper part it drew.

The held rows ride `ui::hang_rows` — the status slot's own count, which every
geometry path already sums (`live_height`, `live_layout`, the cursor seat, the
preview budget, the strip flow) — so the reserved rows and the painted rows
agree by construction; `ui::live::strip_lines` paints them above the status
line, where hanging rows hang below it. `App::strip_hold` applies only while
`ui::layout::holds_strip` says a live strip is up: a turn in flight (the viewed
agent's run inside its session view) with its status line showing, no modal,
and the composer on screen — a picker or the ↓ band lays the strip out by its
own split, and a value injected a frame ago must never pad an idle composer.

Exact rows, not slots: a command's tail owes one row with nothing left in the
preview, and a preview slot cannot be one row (content always brings its gap),
which is why the held rows are a count of their own rather than padding inside
the slot. A purge rebuild re-seats the region, so it starts the tracker over.

## Alternatives considered

- **Drafting cells for every call** — show `● Bash(…)` while its arguments
  stream, the way a `write` previews (`docs/write-streaming.md`). Content
  instead of blank rows, but a new visual state for every tool, with hand-offs
  of its own (a read joining a group, a task call that has no cell, an agent
  launch becoming a tree).
- **An adaptive hold** timed off the session's measured hand-offs. Harder to
  predict and to test than one bound chosen from the measurements, and the
  freeze already covers the case a fixed bound would miss.
- **Easing the release** a row at a time. Motion of its own, and a single
  owed row — the common case between rounds — gains nothing.

## What it does not change

- **The turn's end** collapses the strip into the reply's last line and the
  summary, as before — a settle, not a bounce.
- **A toast expiring** mid-turn lifts the box its one row, one way.
- **A picker or the ↓ band** opened mid-turn keeps its own geometry: the hold
  is the composer path's.
- **Hide thinking** opens no block and no buffer, so nothing waits on a phase.

## Testing

- `ui::tests::strip_hold` plays frames the way the boundary paints them and
  checks the box on each: a thought giving way to its call, to the reply's
  first words and to a read; a command's tail settling (one row held,
  exactly); commits covering the hold; the lapse, and the lapse waiting on an
  open phase; the room clamp; the guards (idle, a modal, a picker, a `!` shell
  turn, a rebuild); the held rows painted blank above the status line; a
  subagent's session view.
- `ui::tests::reasoning`: the block waits for the delay, in the main strip and
  in an agent's session view, and the flow key moves when it appears.
- `scripts/smoke.sh` **Phase 153** samples turns ~20 times a second on the
  real binary — the default offline turn, and the real backend against a
  paced stub provider (a thought then a `bash` call, a command's tail then the
  next round, a quick thought then a `skill` call) — and fails a mid-turn rise
  of the footer confirmed by a second capture (Phase 121's rule), a block on
  screen for a thought shorter than the delay, and held rows that outlive the
  hold.
