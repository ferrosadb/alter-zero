# The hand-off hold — the box holds still while one live cell gives way to the next

## The report

> When the model thinks very fast and calls a tool — the skill tool, say — the
> box jumps and flickers. Not only after thinking: while it is using a tool,
> the next tool call sometimes flickers too.

## What the frames showed

Recorded with `tmux pipe-pane` (each chunk timestamped) and replayed through a
VT emulator one synchronized frame at a time — each frame is everything up to
an `ESC[?2026l`, exactly what a mode-2026 terminal presents at once — with the
footer row read off every frame. Nothing tore; what moved was the box.

The offline demo turn at 100×30 rose twice: two rows for 771 ms when its
thought settled before the batch's cells existed, one row for 444 ms when the
read group's two-row cell settled into its line under the edit behind it.

One real turn (OpenRouter `recursive/flint-v1.0:free`, `medium`, the reported
prompt — load the `web-search` skill, then search) rose **sixteen** times:

| kind | rises | rows | back down after |
| --- | --- | --- | --- |
| a thought settling before the round's call has a cell | 6 | 2–3 | 29, 126, 422, 429, 472 ms; one 3.9 s (a long call being written) |
| a `bash` cell settling into a shorter peek than its live tail | 10 | 1–3 | 1.4–3.5 s (the next round's first token) |

The first kind is the flicker: down, up three rows, down again a frame or a
few later.

## Why

The live region is **content-anchored** (CLAUDE.md invariant 3): a frame that
commits `c` rows to scrollback pushes it down by `c`, and a frame whose region
is `h` rows shorter pulls its bottom up by `h`. So the box rises exactly when
the strip sheds more rows than went to scrollback in the same frame — and
every hand-off between live cells can:

- **A thought settles before its call has a cell.** The backend closes the
  phase at the round's first tool-call piece, and the settle commits the
  `Thought for …` line and a spacer — two rows — while the block stood on
  five (its header, three rows of thought, the gap). The call only gets a cell
  when its batch is announced, once its arguments have streamed. Whether a
  frame catches the gap depends on how fast the pieces arrive, which is why it
  happens *sometimes*.
- **A command settles shorter than it ran.** A running `bash` cell tails four
  rows of output over its `+N lines (Ns · wait …)` clock row; settled, it folds
  to the output's first block over a `… +N lines` hint — one to four rows
  shorter.
- **A read group closes.** Its two-row live cell (`● Reading 1 file…` over the
  hint) settles into one line under the call behind it.

## What earlier fixes taught

Six branches attacked this hand-off before this one; their messages are the
best record of what reads well and what does not.

1. **A floor with no end** (`fix/box-flicker-after-thinking`, then the first
   commit of `fix/thinking-handoff-flicker`, merged and reverted) held the
   slot's rows blank until the next content came. Between rounds that was the
   whole wait for the next request — seconds of three empty lines over the
   status line, *"where there should be one"*.
2. **Settling the thought in place** kept the thought's own tail under its
   settled line, holding the committed line back from scrollback
   (`InlineViewport::hold`) until the call landed. It read as *"a flash of
   stale thought"*, fixed only the thought, and taught the terminal layer to
   hold commits.
3. **Emptied rows under the settled line** (`fix/hide-thought-after-thinking`,
   `fix/settled-thought-line-alone`) still read as *"newlines reserved for
   whatever came next"*, even for the 100–400 ms a call takes to write;
   moving them under the status line made the status line step instead.
4. **Lingering the block** (`fix/thought-linger-instead-of-hold`,
   `fix/thought-handoff-grace`) kept the `● Thinking…` block up, unchanged,
   for a fixed 300 ms or 1 s after the phase ended. It measured the reported
   model: a call's arguments finish 17–120 ms after its thought for a short
   call and 406–642 ms for a command carrying a long path — and, with 300 ms,
   still bounced. It covered only the thought: the command and read-group
   hops stayed, which is the second half of the report.

So: content, not blank rows, where content exists to show; blank rows only
where nothing else can stand in, and only for a moment; and no fixed guess at
how long a call takes to write, when the stream says so.

## The design

Two mechanisms, one policy module (`ui::handoff`).

### The thought lingers

A thinking phase's `ThinkingEnd` stops its clock — `app::ThinkingClock`, so
the status line's `Thinking for Ns` and the settled `Thought for Ns` read the
phase's own length — but leaves the phase **open**, its live block up and
unchanged. It settles, committing its `Thought for … · … tokens` line:

- **with the next content**, in the same frame: the boundary settles it
  *before* folding in any event that draws or records something
  (`StreamEvent::settles_ended_thought`, exhaustive, so a new event has to say
  which side it is on — a call's pieces, blank text, the round's usage frame
  and call ids leave it up), or a piece that opens a previewed `write`. The
  line commits ahead of what the event commits, so history and scrollback keep
  their order (invariant 4), and the call's cell lands on the block's rows;
- **at the draw tick**, once it has waited as long as it may
  (`ui::thought_linger_due`): while the round is still writing a call
  (`ToolDrafts::is_generating` — a piece has streamed, the round is not
  announced) up to `HANDOFF_HOLD_MAX` (3 s), else `HANDOFF_HOLD` (300 ms).
  Nothing took its rows, so the hand-off is a slow one: the hold below starts
  over and the box settles in one move, not over blank rows;
- **on an Esc, an error or a quit**, as before (a quit settles it, so a
  finished thought still reaches scrollback and the rollout); a retry drops it
  with its void attempt.

Two details follow. The round's usage frame arrives before its batch, so it
lands while the phase is open: its `reasoning_tokens` wait in
`App::round_reasoning_due` until the phase is recorded, so the committed line
shows the provider's count and the next round's thought never inherits it.
And blank text buffered while the block lingered is the start of the reply
after the thought, not text before it, so the settle leaves it in the buffer.

No block is shown — so none lingers — inside an open read/search group (the
thought settles into the group unseen, `docs/tool-groups.md`), with the
display off, or in a compact turn. A **subagent's session view** keeps the
settle-at-once and relies on the hold below: its fold runs on the agent's own
channel and clocks, and the hold already keeps its box still.

### Every other hand-off holds its rows

**The strip keeps the rows it stood on, minus what committed, for as long as
the next cell is plausibly about to land — and not a moment longer.**

The tracker is `ui::HandoffHold`, owned by the boundary like `StreamRender`.
Each conversation-view frame:

1. The boundary zeroes the injected hold, so everything the frame measures
   before deciding is the strip's own content.
2. `HandoffHold::rows(app, committed, width, height, now)` decides the rows to
   hold — `committed` being the rows queued for scrollback since the last
   painted frame (`InlineViewport::pending_rows`), `now` the boundary's clock.
3. `App::set_handoff_hold` injects them; the frame paints;
   `HandoffHold::record` remembers what the preview slot stood on.

The decision is the slot's arithmetic, then a clock:

- **What is owed.** The slot (the preview content, its gap, the held rows)
  stood on `s` rows last frame and `c` rows committed since, so the frame
  owes `s − c` rows. When the content's own slot covers that, nothing is
  held.
- **How long.** Each hand-off gets its own clock — it starts on the first
  frame that has to hold a row, and again on any frame that commits rows and
  still owes some (a cell giving way to the next is a new hand-off, not the
  old one running on) — and the hold lets go once it is older than
  `hold_limit`:
  - `HANDOFF_HOLD_MAX` (3 s) while something is **certainly on its way**: a
    call being written (its cell lands with the round's announcement), or the
    reply's text streaming (its next line commits and pays the hold off);
  - `HANDOFF_HOLD_RUN` (1 s) while a **tool is running**: its settle commits
    rows that pay the hold off in the same frame, so a quick one — a read, an
    edit, a skill load, an `ls` — leaves the box where it was; a command can
    run for minutes, so the wait is only a moment;
  - `HANDOFF_HOLD` (300 ms) otherwise — an empty strip, a read group idling
    between rounds: the next cell is the next request's.
- **Where.** Commits pay it off row for row, so a hold never outlives the
  rows it stands for; it never takes rows the preview's budget does not have
  (a short terminal keeps its composer, and held rows never flow); nothing is
  held outside a turn, in a `!` shell turn, or under a modal prompt; a purge
  rebuild starts it over.

**The held rows sit above the status line**, inside the status slot —
`ui::hang_rows` counts them, the one sum the strip geometry sizes by, so the
region, the layout, the cursor seat and the flow check all agree with no new
term — and `strip_lines` paints them as blank rows directly over the status
line. So the status line and the box stand still while the content above them
changes: a settled command's cell commits and the next round's thinking block
grows into the rows it left. Counting them in the status slot, rather than as
preview rows, holds a single row exactly: a preview row brings its gap, so the
slot cannot hold one row on its own.

### Why these lengths

A fixed hold only moves the flicker unless its limit sits where hand-offs do
not: a release at `g` followed by the next cell at `g + ε` is the same
up-and-down, just later. The gaps inside a round are a frame or two, and the
gaps between rounds are the next request's first token — rarely under half a
second on a hosted model, 0.9–3.5 s on the reported one. 300 ms covers the
first and releases well before the second, so the rise it cannot avoid is a
clean one, long before the next round lands. (A longer grace is worse, not
better: the reported model's quickest next round came 0.86 s after its
command settled, so a 600 ms grace would have let go 260 ms before it — a
bob, where 300 ms leaves the box up for over half a second.)

The longer waits are for gaps known to end in a commit. A first cut waited
the full 3 s under *any* live cell, and a long real turn showed what that
costs: three seconds of blank rows under a read group idling between rounds,
and under a command that ran on — then the rise anyway. Hence the tiers: the
cap only where something is certainly coming, a moment for a running tool,
the grace for the rest.

### What it does not change

- **Between rounds on a slow model** a command whose live tail stood taller
  than its peek still lets the box rise — 300 ms after the command ends
  rather than at once — and the next round's first block pushes it back down,
  half a second or more later. Holding the rows through the whole wait is
  the floor that was rejected.
- **The turn's end** collapses the strip into the reply's last rows and the
  `Done for Ns` summary as before: no hold outlives the turn.
- Nothing about what is committed, or in what order: the hold is geometry
  only, and the linger moves the thought's commit to the frame its successor
  lands in without moving it in history.

## Testing

- `ui::tests::handoff_hold` plays frames the way the boundary paints them — ask
  the tracker, inject, measure the region, record — over each hand-off: a
  thought giving way to its call or to the reply, a command's tall tail
  settling, a read group closing under a running edit (held until the edit's
  own cell pays it off), a single-row hold held exactly, each wait's limit
  (`hold_limit` over every state: a call being written, the reply streaming,
  a call waiting or running, an idle open group, an empty strip) and the
  linger's (`thought_linger_due`), commits paying the hold off, and the
  guards (idle, a shell turn, a modal, the budget, a composer growing under
  the hold, a stale injection, a reset, the agent session view).
  `strip_lines` paints the held rows over the status line, and the status
  line keeps its row across the hand-off. Every guard was mutation-checked:
  removing it fails a test.
- `stream::tests::event` pins which events settle a lingering thought;
  `app::tests::reasoning` the phase clock (frozen at its end) and the usage
  frame waiting for the thought; `app::tests::drafts` when a call is being
  generated.
- `scripts/smoke.sh` **Phase 153** samples the footer row ~20 times a second
  (Phase 121's double-capture rule) through the demo turn on a tall terminal
  and on a short one where the box sits on the bottom row, and through the
  real backend against a paced stub: a thought, then a `bash` call written
  over about a second, then a three-second round boundary. No sample lifts
  the footer; no blank row is reserved over the status line before the call's
  cell lands, and the settled line never stands over its stale thought; and
  between rounds the held rows are gone long before the next request answers.
  It fails on the pre-fix binary (the hops), with the linger turned off (627–
  933 ms of reserved rows) and with the hold left unbounded (2.9 s of blank
  rows at the round boundary). Phase 66 reads the thought's Ctrl+O expansion
  from the top, since the round's calls now land in the frame its line
  settles.
- Real provider, recorded with `tmux pipe-pane` and replayed frame by frame
  (100×30, OpenRouter `recursive/flint-v1.0:free` at `medium`):

  | turn | box rises before | after |
  | --- | --- | --- |
  | the reported prompt (skill, then searches) | 16 | 0 |
  | skill → `uname` → `date` → read → reply | — | 0 (no frame with a reserved row) |
  | the offline demo turn, 100×30 and 80×16 | 2 | 0 |

  A long search-heavy turn (~90 s, many rounds) still lets the box rise
  between rounds where a command settled shorter than it ran — 1 to 4 rows,
  each a second or more before the next round lands.
