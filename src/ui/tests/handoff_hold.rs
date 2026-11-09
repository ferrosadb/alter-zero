//! The strip's **hand-off hold** (`docs/handoff-hold.md`): mid-turn, the box
//! holds still while one live cell gives way to the next — a settled thought
//! before its call has a cell, a command settling shorter than its live tail,
//! a read group closing — for as long as the next cell is plausibly about to
//! land, and not a moment longer.
//!
//! Each test plays frames the way the boundary paints them: zero the injected
//! hold, ask the tracker for this frame's rows given the rows committed since
//! the last one and the clock, inject them, measure the region, record.

use std::time::{Duration, Instant};

use super::*;
use crate::stream::{ToolCallFragment, ToolCallSummary};
use crate::ui::handoff::hold_limit;
use crate::ui::layout::{live_height_for, live_rows};
use crate::ui::live::strip_lines;
use crate::ui::theme::{HANDOFF_HOLD, HANDOFF_HOLD_MAX, HANDOFF_HOLD_RUN};

const W: u16 = 80;
const H: u16 = 30;

/// The composer path's region height for `app` at `height` rows — the
/// boundary's `live_region_height` with no composer-replacing view open.
fn region_at(app: &App, height: u16) -> u16 {
    live_height_for(&app.input, W, height, &live_rows(app, W, height))
}

fn region(app: &App) -> u16 {
    region_at(app, H)
}

/// The boundary's frame on a terminal `height` rows tall: zero the injected
/// hold (so everything measured before deciding is the strip's own
/// content), ask the tracker, inject, measure, record. Returns the region's
/// height.
fn frame_at(
    app: &mut App,
    hold: &mut HandoffHold,
    committed: usize,
    now: Instant,
    height: u16,
) -> u16 {
    app.set_handoff_hold(0);
    let rows = hold.rows(app, committed, W, height, now);
    app.set_handoff_hold(rows);
    let painted = region_at(app, height);
    hold.record(app, W, height);
    painted
}

fn frame(app: &mut App, hold: &mut HandoffHold, committed: usize, now: Instant) -> u16 {
    frame_at(app, hold, committed, now, H)
}

/// The invariant the hold exists for: a frame that committed `committed` rows
/// lands the region's bottom exactly where the frame before it had it —
/// neither up (the reported hop) nor down.
#[track_caller]
fn assert_still(before: u16, after: u16, committed: usize, what: &str) {
    assert_eq!(
        usize::from(after) + committed,
        usize::from(before),
        "{what}: the box moved — the region went {before} → {after} rows with \
         {committed} committed above it"
    );
}

/// A turn thinking `text`, its block in the strip.
fn thinking(text: &str) -> App {
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking(text);
    app
}

/// Settle the open phase and return the rows the boundary commits for it —
/// the `Thought for …` line and its spacer.
fn settle(app: &mut App) -> usize {
    let thought = app.finish_reasoning(0).expect("a thought to settle");
    reasoning_lines(&thought, W).len() + 1
}

/// The rows the boundary commits for the call just resolved — its cell and
/// the spacer (0 when it joined the open read group).
fn commit_resolved(app: &mut App) -> usize {
    if app.place_resolved_tool() == crate::app::ToolPlacement::Held {
        return 0;
    }
    tool_commit_lines(&app.history, app.tool_queue(), W, app.path_display())
        .map_or(0, |lines| lines.len() + 1)
}

/// One streamed piece of the round's call `index`.
fn piece(index: usize, name: &str, arguments: &str) -> ToolCallFragment {
    ToolCallFragment {
        index,
        name: name.to_string(),
        arguments: arguments.to_string(),
    }
}

fn summary(name: &str, args: &str) -> ToolCallSummary {
    ToolCallSummary {
        name: name.into(),
        args: args.into(),
    }
}

/// The composer box's rows in the region `app` paints on a terminal
/// `height` rows tall.
fn box_rows(app: &App, height: u16) -> u16 {
    let area = Rect {
        x: 0,
        y: 0,
        width: W,
        height: region_at(app, height),
    };
    crate::ui::layout::live_layout_for(area, &live_rows(app, W, height))[1].height
}

/// The row of the status line in the painted strip, counted up from the
/// strip's bottom — what keeps it standing over the box.
fn status_from_bottom(app: &App) -> usize {
    let lines = strip_lines(app, W, None, fitted_preview_rows(app, W, H));
    let at = lines
        .iter()
        .position(|line| plain(line).contains("esc to interrupt"))
        .expect("a status line in the strip");
    lines.len() - at
}

#[test]
fn a_settled_thought_holds_the_box_still_until_the_call_lands() {
    // The reported flicker: think → the Skill call. The block settles at the
    // call's first piece, but the call's cell only arrives with the batch, so
    // the box used to sit three rows up for as long as the call took to
    // generate — then drop back when the cell landed.
    let t0 = Instant::now();
    let mut app = thinking("The user wants the champion.\nUse the skill first.\nThen search.");
    let mut hold = HandoffHold::default();
    let block = frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    let held = frame(&mut app, &mut hold, committed, t0);
    assert_still(block, held, committed, "the thought settled");
    // The call streams in: nothing commits, nothing moves.
    app.push_tool_call_fragment(&piece(0, "skill", ""));
    let ms = Duration::from_millis;
    assert_eq!(frame(&mut app, &mut hold, 0, t0 + ms(40)), held);
    app.push_tool_call_fragment(&piece(0, "", r#"{"skill":"web-search"}"#));
    assert_eq!(frame(&mut app, &mut hold, 0, t0 + ms(80)), held);
    // The round is announced: the cell lands in the held rows, exactly.
    app.open_round(Vec::new());
    app.start_tool_batch(&[summary("Skill", "web-search")]);
    assert_eq!(
        frame(&mut app, &mut hold, 0, t0 + ms(90)),
        held,
        "the cell lands in the rows the thought left"
    );
    assert_eq!(app.handoff_hold(), 0, "nothing is held once the cell is up");
}

#[test]
fn the_status_line_keeps_its_row_through_the_hand_off() {
    // Held rows are painted over the status line, inside its slot: the
    // thought's tail gives way to blank rows and the status line and the box
    // stay exactly where they were.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let before = status_from_bottom(&app);
    let committed = settle(&mut app);
    frame(&mut app, &mut hold, committed, t0);
    assert!(app.handoff_hold() > 0, "the settle held rows");
    assert_eq!(
        status_from_bottom(&app),
        before,
        "the status line stays put over the box"
    );
    let lines = strip_lines(&app, W, None, fitted_preview_rows(&app, W, H));
    let status = lines.len() - before;
    let held = usize::from(app.handoff_hold());
    assert!(
        lines[status - held..status]
            .iter()
            .all(|line| plain(line).trim().is_empty()),
        "the held rows are blank, directly over the status line: {:?}",
        lines.iter().map(plain).collect::<Vec<_>>()
    );
    assert_eq!(
        lines.len(),
        usize::from(crate::ui::layout::strip_paint_rows(&app, W, H)),
        "the strip paints exactly the rows the region reserved for it"
    );
}

#[test]
fn a_thought_giving_way_to_the_reply_holds_under_its_first_row() {
    // Think → answer: the block settles as the reply's first token arrives,
    // and a one-row frontier used to leave the box rows higher until the
    // reply's lines committed.
    let t0 = Instant::now();
    let mut app = thinking("Today is the 7th.\nThe match starts on the 24th.\nAnswer.");
    let mut hold = HandoffHold::default();
    let block = frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    app.push_chunk("**Classical");
    app.set_stream_preview_rows(1);
    let after = frame(&mut app, &mut hold, committed, t0);
    assert_still(block, after, committed, "the thought gave way to the reply");
    // The reply's first line commits: the hold drops by what committed.
    let next = frame(&mut app, &mut hold, 1, t0 + Duration::from_millis(20));
    assert_still(after, next, 1, "the reply's first line committed");
}

#[test]
fn a_command_settling_shorter_than_its_tail_holds_its_rows() {
    // A running command tails four rows over its `+N lines` clock row;
    // settled, it folds to its first block over a hint — shorter. The box
    // rose by the difference, then fell back when the next round began.
    let t0 = Instant::now();
    let mut app = App::new();
    app.begin_stream();
    app.start_tool(
        "Bash",
        "python3 search.py",
        Some(r#"{"command":"python3 search.py"}"#),
    );
    app.set_command_elapsed(Some(Duration::from_secs(2)));
    let output = "## yahoo (3 results)\n\n1. one\n2. two\n3. three\n4. four\n5. five\n6. six";
    app.push_tool_output(output);
    let mut hold = HandoffHold::default();
    let running = frame(&mut app, &mut hold, 0, t0);
    app.end_tool(output, true).expect("a running call");
    let committed = commit_resolved(&mut app);
    let settled = frame(&mut app, &mut hold, committed, t0);
    assert_still(running, settled, committed, "the command settled");
    assert!(
        app.handoff_hold() > 0,
        "its tail stood taller than its peek"
    );
}

#[test]
fn a_single_row_is_held_exactly() {
    // A short output fits the live window, over the bare clock row the
    // settled cell drops: one row short. A preview row brings its gap, so the
    // hold lives in the status slot, where one row is one row.
    let t0 = Instant::now();
    let mut app = App::new();
    app.begin_stream();
    app.start_tool(
        "Bash",
        "python3 build.py",
        Some(r#"{"command":"python3 build.py"}"#),
    );
    app.set_command_elapsed(Some(Duration::from_secs(1)));
    app.push_tool_output("built a.txt\nbuilt b.txt");
    let mut hold = HandoffHold::default();
    let running = frame(&mut app, &mut hold, 0, t0);
    app.end_tool("built a.txt\nbuilt b.txt", true)
        .expect("a running call");
    let committed = commit_resolved(&mut app);
    let settled = frame(&mut app, &mut hold, committed, t0);
    assert_eq!(app.handoff_hold(), 1, "one row owed, one row held");
    assert_still(running, settled, committed, "the short command settled");
}

#[test]
fn the_hold_lets_go_after_its_grace() {
    // Between rounds the next cell is a whole request away: the rows are held
    // for HANDOFF_HOLD, then the strip collapses to its content — never the
    // seconds of empty lines over the status line the first floor held.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    let held = frame(&mut app, &mut hold, committed, t0);
    let almost = HANDOFF_HOLD - Duration::from_millis(1);
    assert_eq!(
        frame(&mut app, &mut hold, 0, t0 + almost),
        held,
        "still held"
    );
    let released = frame(&mut app, &mut hold, 0, t0 + HANDOFF_HOLD);
    assert_eq!(app.handoff_hold(), 0, "the grace ran out");
    assert!(released < held, "the strip collapsed to its content");
    // …and stays collapsed: what was released is not held again.
    assert_eq!(
        frame(&mut app, &mut hold, 0, t0 + HANDOFF_HOLD * 2),
        released
    );
}

#[test]
fn a_call_being_written_extends_the_hold_up_to_its_cap() {
    // A long call is still on its way while its pieces arrive: the hold waits
    // for its cell past the grace — but not forever.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    let held = frame(&mut app, &mut hold, committed, t0);
    let rows = app.handoff_hold();
    assert!(rows > 0, "the settle held rows");
    app.push_tool_call_fragment(&piece(0, "bash", r#"{"command":"for i in"#));
    let later = HANDOFF_HOLD + Duration::from_millis(500);
    assert_eq!(
        frame(&mut app, &mut hold, 0, t0 + later),
        held,
        "held past the grace while the call streams"
    );
    assert_eq!(app.handoff_hold(), rows);
    let almost = HANDOFF_HOLD_MAX - Duration::from_millis(1);
    assert_eq!(frame(&mut app, &mut hold, 0, t0 + almost), held);
    assert_eq!(app.handoff_hold(), rows, "still held just short of the cap");
    frame(&mut app, &mut hold, 0, t0 + HANDOFF_HOLD_MAX);
    assert_eq!(app.handoff_hold(), 0, "the cap lets a very long call go");
}

#[test]
fn an_announced_round_stops_extending_the_hold() {
    // A round of task calls records no cell (docs/task-tools.md): once it is
    // announced nothing will land on the held rows, so only the grace holds.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    frame(&mut app, &mut hold, committed, t0);
    app.push_tool_call_fragment(&piece(0, "taskcreate", r#"{"subject":"x"}"#));
    let short = HANDOFF_HOLD - Duration::from_millis(1);
    frame(&mut app, &mut hold, 0, t0 + short);
    assert!(app.handoff_hold() > 0, "held while the call streams");
    app.open_round(Vec::new());
    frame(&mut app, &mut hold, 0, t0 + HANDOFF_HOLD);
    assert_eq!(app.handoff_hold(), 0, "an announced round holds no longer");
}

#[test]
fn commits_pay_the_hold_off_row_for_row() {
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    let held = frame(&mut app, &mut hold, committed, t0);
    let rows = app.handoff_hold();
    assert!(rows >= 2, "a three-row thought holds more than a row");
    let next = frame(&mut app, &mut hold, 1, t0);
    assert_eq!(
        app.handoff_hold(),
        rows - 1,
        "one committed row, one fewer held"
    );
    assert_still(held, next, 1, "a commit under the hold");
    frame(&mut app, &mut hold, usize::from(rows), t0);
    assert_eq!(app.handoff_hold(), 0, "commits covered the rest");
}

#[test]
fn the_read_group_settling_under_the_next_call_does_not_lift_the_box() {
    // A read, then an edit: the group's two-row live cell settles into one
    // `Read 1 file` line while the edit runs on, a row short of what the
    // strip had held.
    let t0 = Instant::now();
    let mut app = App::new();
    app.begin_stream();
    app.start_tool_batch(&[summary("Read", "about.py"), summary("Edit", "about.py")]);
    let mut hold = HandoffHold::default();
    app.start_tool("Read", "about.py", None);
    frame(&mut app, &mut hold, 0, t0);
    app.end_tool("1 x", true).expect("a running call");
    assert_eq!(commit_resolved(&mut app), 0, "the read joins the group");
    let grouped = frame(&mut app, &mut hold, 0, t0);
    app.start_tool("Edit", "about.py", None);
    let range = app.close_tool_group().expect("the group was open");
    let line = tool_group_lines(&app.history[range], W, app.path_display()).len() + 1;
    let after = frame(&mut app, &mut hold, line, t0);
    assert_still(grouped, after, line, "the group settled under the edit");
}

#[test]
fn nothing_is_held_once_the_turn_is_over() {
    // The turn's end collapses the whole strip into its summary — never a
    // hold, or the idle composer would sit under blank rows.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    settle(&mut app);
    app.finish_stream();
    app.take_turn_summary(3);
    assert!(!app.turn_active());
    frame(&mut app, &mut hold, 0, t0);
    assert_eq!(app.handoff_hold(), 0, "idle holds nothing");
}

#[test]
fn an_injected_hold_counts_only_while_a_live_strip_is_up() {
    // A value injected a frame ago can never pad an idle composer.
    let mut app = App::new();
    let idle = hang_rows(&app, W);
    app.set_handoff_hold(3);
    assert_eq!(hang_rows(&app, W), idle, "idle: the injection is ignored");
    app.begin_stream();
    let live = region(&app);
    app.set_handoff_hold(0);
    assert_eq!(region(&app) + 3, live, "mid-turn the held rows count");
}

#[test]
fn nothing_is_held_in_a_shell_turn() {
    // A `!` turn has no status line to hold rows over — its one row is the
    // `⎿ Running… (Ns)` preview, and its end is the turn's end.
    let t0 = Instant::now();
    let mut app = App::new();
    app.begin_shell("ls");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    app.set_handoff_hold(4);
    assert_eq!(
        hold.rows(&app, 0, W, H, t0),
        0,
        "a shell turn holds nothing"
    );
}

#[test]
fn nothing_is_held_under_a_modal_prompt() {
    // A permission prompt replaces the whole region; its close
    // purge-rebuilds. Nothing of the strip's geometry carries across.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    app.open_permission(crate::permission::PermissionRequest {
        id: "perm_0".to_string(),
        kind: crate::permission::PermissionKind::Write,
        target: "hello.py".to_string(),
        body: "1 print(\"hi\")".to_string(),
        detail: None,
        agent: None,
        agent_id: None,
    });
    frame(&mut app, &mut hold, committed, t0);
    assert_eq!(app.handoff_hold(), 0, "a modal holds nothing");
    // The user answers Yes: the prompt closes, the turn runs on, and what
    // the modal frame recorded holds nothing after it.
    let yes = ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('1'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    );
    app.on_key(yes);
    assert!(!app.modal_open() && app.turn_active());
    frame(&mut app, &mut hold, 0, t0);
    assert_eq!(app.handoff_hold(), 0);
}

#[test]
fn the_hold_never_takes_rows_the_terminal_does_not_have() {
    // On a short terminal the slot's budget is what the composer leaves: a
    // hold past it would squeeze the box, and blank rows taller than the
    // strip's rect would flow into scrollback with a purge rebuild. Swept over
    // heights where the thought's block is squeezed, fits exactly, and fits
    // with room to spare.
    for short in 6..=14 {
        let t0 = Instant::now();
        let mut app = thinking("one\ntwo\nthree");
        let mut hold = HandoffHold::default();
        frame_at(&mut app, &mut hold, 0, t0, short);
        let committed = settle(&mut app);
        frame_at(&mut app, &mut hold, committed, t0, short);
        let painted = usize::from(crate::ui::layout::strip_paint_rows(&app, W, short));
        let built = strip_lines(&app, W, None, fitted_preview_rows(&app, W, short)).len();
        assert_eq!(
            built,
            painted,
            "at {short} rows the strip holds {} rows it has no room for",
            app.handoff_hold()
        );
        assert!(
            crate::ui::view_flow(&app, W, short, 10_000).is_none(),
            "at {short} rows held rows never flow"
        );
    }
}

#[test]
fn a_reset_starts_the_hold_over() {
    // A purge rebuild re-seats the region from scratch: there is no bottom
    // left to hold.
    let t0 = Instant::now();
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame(&mut app, &mut hold, 0, t0);
    let committed = settle(&mut app);
    hold.reset();
    frame(&mut app, &mut hold, committed, t0);
    assert_eq!(app.handoff_hold(), 0, "a rebuild re-seats the region");
}

#[test]
fn a_viewed_agents_thought_holds_its_rows_too() {
    // The agent session view streams the agent's own `● Thinking…` block and
    // settles it the same way (docs/agent-view-streaming.md), so the same hop
    // lived there.
    use crate::stream::StreamEvent;
    let t0 = Instant::now();
    let mut app = App::new();
    app.begin_stream();
    app.start_agent_group(false, &[spec("a1", "Search", false)]);
    app.begin_agent_reasoning("a1");
    app.apply_agent_event("a1", &StreamEvent::ThinkingChunk("one\ntwo\nthree".into()));
    app.open_agent_view("a1");
    let mut hold = HandoffHold::default();
    let block = frame(&mut app, &mut hold, 0, t0);
    let thought = app.finish_agent_reasoning("a1", 0).expect("a thought");
    let committed = reasoning_lines(&thought, W).len() + 1;
    let after = frame(&mut app, &mut hold, committed, t0);
    assert_still(block, after, committed, "the agent's thought settled");
}

#[test]
fn the_hold_is_decided_from_the_strips_own_rows() {
    // The rows injected for the last frame sit in the status slot, which the
    // preview's budget is measured against — a decision that counted them
    // would feed on itself. The answer is the same whether the boundary has
    // zeroed the injection yet or not, on every terminal height.
    for short in 8..=16 {
        let t0 = Instant::now();
        let mut app = thinking("one\ntwo\nthree");
        let mut hold = HandoffHold::default();
        frame_at(&mut app, &mut hold, 0, t0, short);
        let committed = settle(&mut app);
        frame_at(&mut app, &mut hold, committed, t0, short);
        let held = app.handoff_hold();
        let (mut zeroed, mut stale) = (hold, hold);
        app.set_handoff_hold(0);
        let fresh = zeroed.rows(&app, 0, W, short, t0);
        app.set_handoff_hold(held);
        assert_eq!(
            stale.rows(&app, 0, W, short, t0),
            fresh,
            "at {short} rows the stale injection changed the decision"
        );
    }
}

#[test]
fn a_hold_gives_way_to_a_composer_that_grows_under_it() {
    // The user types on while the rows are held, and the message wraps onto
    // more rows: the region's other rows grow and the room the slot may take
    // shrinks under the hold. The hold yields — the strip never asks for rows
    // the terminal no longer has (which would flow blank rows into
    // scrollback with a purge rebuild) — and the composer keeps every row.
    let t0 = Instant::now();
    let short = 12;
    let mut app = thinking("one\ntwo\nthree");
    let mut hold = HandoffHold::default();
    frame_at(&mut app, &mut hold, 0, t0, short);
    let committed = settle(&mut app);
    frame_at(&mut app, &mut hold, committed, t0, short);
    assert!(app.handoff_hold() > 0, "the settle held rows");
    app.input = TextArea::from_text(&"a word that wraps ".repeat(30));
    frame_at(&mut app, &mut hold, 0, t0, short);
    let held = app.handoff_hold();
    let with_hold = box_rows(&app, short);
    app.set_handoff_hold(0);
    assert_eq!(
        box_rows(&app, short),
        with_hold,
        "the composer keeps every row it had — {held} held rows took none"
    );
    app.set_handoff_hold(held);
    let painted = usize::from(crate::ui::layout::strip_paint_rows(&app, W, short));
    let built = strip_lines(&app, W, None, fitted_preview_rows(&app, W, short)).len();
    assert_eq!(built, painted, "the strip asks only for rows it has");
    assert!(
        crate::ui::view_flow(&app, W, short, 10_000).is_none(),
        "held rows never flow"
    );
}

#[test]
fn a_hold_under_a_live_cell_waits_for_that_cell_to_settle() {
    // The read group settled into its line while the edit behind it runs on.
    // Letting the held row go on the short grace lifted the box a beat before
    // the edit's own cell committed and pushed it back down — a quicker
    // flicker than holding nothing. While a live cell is on screen its settle
    // is what pays the hold off, in the same frame, so the hold waits for it
    // (up to the cap).
    let t0 = Instant::now();
    let (mut app, mut hold, held) = group_closed_under_an_edit(t0);
    let later = HANDOFF_HOLD + Duration::from_millis(150);
    assert_eq!(
        frame(&mut app, &mut hold, 0, t0 + later),
        held,
        "the edit is still running: the row stays held"
    );
    // The edit settles: its cell commits in the same frame the hold is paid
    // off, so the box never rose — and whatever the commit did not cover is
    // a new hand-off with a grace of its own, not the old one's leftover.
    app.end_tool("Updated about.py (+1 -1)", true)
        .expect("a running call");
    let cell = commit_resolved(&mut app);
    let settled = frame(&mut app, &mut hold, cell, t0 + later);
    assert_still(held, settled, cell, "the edit's cell committed");
}

/// A read, then an edit: the read joins the group, then the edit starts and
/// the group settles into its line above it — a row short of what the strip
/// had held. Returns the app mid-edit, its tracker, and the held region.
fn group_closed_under_an_edit(t0: Instant) -> (App, HandoffHold, u16) {
    let mut app = App::new();
    app.begin_stream();
    app.start_tool_batch(&[summary("Read", "about.py"), summary("Edit", "about.py")]);
    let mut hold = HandoffHold::default();
    app.start_tool("Read", "about.py", None);
    frame(&mut app, &mut hold, 0, t0);
    app.end_tool("1 x", true).expect("a running call");
    assert_eq!(commit_resolved(&mut app), 0, "the read joins the group");
    frame(&mut app, &mut hold, 0, t0);
    app.start_tool("Edit", "about.py", None);
    let range = app.close_tool_group().expect("the group was open");
    let line = tool_group_lines(&app.history[range], W, app.path_display()).len() + 1;
    let held = frame(&mut app, &mut hold, line, t0);
    assert!(app.handoff_hold() > 0, "the group's line left a row owed");
    (app, hold, held)
}

#[test]
fn a_running_call_that_never_settles_lets_go_at_its_cap() {
    // A quick tool settles inside the wait and pays the hold off; a long
    // command does not, and blank rows held under it for seconds are the
    // rejected floor's — so a run is waited for only a moment.
    let t0 = Instant::now();
    let (mut app, mut hold, held) = group_closed_under_an_edit(t0);
    let almost = HANDOFF_HOLD_RUN - Duration::from_millis(1);
    assert_eq!(frame(&mut app, &mut hold, 0, t0 + almost), held);
    frame(&mut app, &mut hold, 0, t0 + HANDOFF_HOLD_RUN);
    assert_eq!(app.handoff_hold(), 0, "the cap lets a long run go");
}

#[test]
fn how_long_a_hold_waits_follows_what_is_on_its_way() {
    // Something certainly coming — a call's pieces, the reply's text — is
    // waited for up to the cap; a tool running only for a moment, since a
    // command can run for minutes; anything else (an empty strip, a read
    // group idling between rounds) only the short grace.
    let mut app = App::new();
    app.begin_stream();
    assert_eq!(hold_limit(&app), HANDOFF_HOLD, "an empty strip");
    app.push_tool_call_fragment(&piece(0, "bash", r#"{"command":"ls"#));
    assert_eq!(hold_limit(&app), HANDOFF_HOLD_MAX, "a call being written");
    app.open_round(Vec::new());
    app.start_tool_batch(&[summary("Bash", "python3 build.py")]);
    assert_eq!(hold_limit(&app), HANDOFF_HOLD, "a call waiting to run");
    app.start_tool(
        "Bash",
        "python3 build.py",
        Some(r#"{"command":"python3 build.py"}"#),
    );
    assert_eq!(hold_limit(&app), HANDOFF_HOLD_RUN, "a call running");
    app.end_tool("built", true).expect("a running call");
    commit_resolved(&mut app);
    app.push_chunk("The build");
    assert_eq!(hold_limit(&app), HANDOFF_HOLD_MAX, "the reply streaming");
    // A read group left open between rounds, its reads resolved: nothing
    // is on its way until the next request answers.
    let mut idle = App::new();
    idle.begin_stream();
    idle.start_tool_batch(&[summary("Read", "about.py")]);
    idle.start_tool("Read", "about.py", None);
    idle.end_tool("1 x", true).expect("a running call");
    assert_eq!(commit_resolved(&mut idle), 0, "the read joins the group");
    assert!(idle.tool_group_range().is_some(), "the group stays open");
    assert_eq!(hold_limit(&idle), HANDOFF_HOLD, "an idle open group");
}

// --- the thought lingers (docs/handoff-hold.md *The thought lingers*) ---

#[test]
fn an_ended_thought_lingers_while_its_call_is_written_and_briefly_otherwise() {
    // The backend ends a phase at the round's first tool-call piece; its live
    // block stays up until the call's cell takes its rows. While the call's
    // pieces arrive its cell is on its way, so the block waits for it — up to
    // the cap; with nothing on its way it waits only the short grace.
    let ms = Duration::from_millis;
    let mut app = thinking("one\ntwo\nthree");
    assert!(!crate::ui::thought_linger_due(&app, ms(0)));
    assert!(
        !crate::ui::thought_linger_due(&app, HANDOFF_HOLD - ms(1)),
        "inside the grace"
    );
    assert!(
        crate::ui::thought_linger_due(&app, HANDOFF_HOLD),
        "nothing is on its way: the grace is all it waits"
    );
    app.push_tool_call_fragment(&piece(0, "bash", r#"{"command":"for"#));
    assert!(
        !crate::ui::thought_linger_due(&app, HANDOFF_HOLD + ms(500)),
        "the call is being written: its cell is on its way"
    );
    assert!(!crate::ui::thought_linger_due(
        &app,
        HANDOFF_HOLD_MAX - ms(1)
    ));
    assert!(
        crate::ui::thought_linger_due(&app, HANDOFF_HOLD_MAX),
        "a very long call still lets the thought settle at the cap"
    );
}

#[test]
fn a_lingering_thought_waits_on_the_main_conversations_call_in_an_agent_view() {
    // Only the main conversation's thought lingers, so whether its call is
    // still being written is asked of the main turn — not of the agent whose
    // session view happens to be on screen.
    let ms = Duration::from_millis;
    let mut app = thinking("one\ntwo\nthree");
    app.start_agent_group(false, &[spec("a1", "Search", false)]);
    app.open_agent_view("a1");
    app.push_tool_call_fragment(&piece(0, "bash", r#"{"command":"for"#));
    assert!(
        !crate::ui::thought_linger_due(&app, HANDOFF_HOLD + ms(500)),
        "the main turn's call is being written: its thought waits for it"
    );
}
