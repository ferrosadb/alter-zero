//! The strip hold (`docs/strip-hold.md`): a hand-off between two live cells
//! never lifts the box while the next one is moments away.
//!
//! The frames are played the way `Session::draw_conversation` paints them —
//! the hold measured with the rows committed since the last frame, injected,
//! the frame painted, what it drew recorded — and each test reads where the
//! box went. The region is content-anchored (CLAUDE.md invariant 3): a frame
//! that commits `c` rows pushes the box down by `c`, and a region `h` rows
//! shorter pulls it up by `h`, so the box moved by `c + Δh`.

use super::*;
use crate::permission::{PermissionKind, PermissionRequest};
use crate::stream::{StreamEvent, ToolCallSummary};
use crate::ui::layout::{live_height_for, live_rows, strip_rows};
use crate::ui::theme::{INPUT_CHROME_ROWS, REASONING_SHOW_DELAY, STRIP_HOLD};

const W: u16 = 80;
const H: u16 = 40;

/// The live region's height the next draw would use — the composer-path sum
/// `tui::view::live_region_height` ends on.
fn region(app: &App, height: u16) -> u16 {
    let band = band_rows(app, W);
    live_height(
        &app.input,
        W,
        height,
        strip_has_status(app),
        fitted_preview_rows(app, W, height),
        hang_rows(app, W),
        queued_rows(app, W),
        toast_rows(app),
        band,
        footer_rows(app, band),
        agent_list_rows(app),
    )
}

/// A terminal painting frames: the hold the boundary owns, the region's last
/// height, and the loop clock the hold is timed on.
struct Screen {
    hold: StripHold,
    height: u16,
    term: u16,
    now: Duration,
}

impl Screen {
    /// The first frame of `app` on a `term`-row terminal.
    fn new(app: &mut App, term: u16) -> Self {
        let mut screen = Self {
            hold: StripHold::default(),
            height: 0,
            term,
            now: Duration::from_secs(100),
        };
        screen.paint(app, 0);
        screen
    }

    /// Paint one frame after `committed` rows were queued for scrollback,
    /// returning how far the box moved — down positive.
    fn paint(&mut self, app: &mut App, committed: usize) -> i32 {
        let rows = self.hold.rows(app, committed, W, self.term, self.now);
        app.set_strip_hold(rows);
        self.hold.painted(app, W, self.term);
        let height = region(app, self.term);
        let moved =
            i32::try_from(committed).expect("small") + i32::from(height) - i32::from(self.height);
        self.height = height;
        moved
    }

    fn wait(&mut self, by: Duration) {
        self.now += by;
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn summary(name: &str, args: &str) -> ToolCallSummary {
    ToolCallSummary {
        name: name.to_string(),
        args: args.to_string(),
    }
}

/// A turn whose thinking phase is showing its block — three rows of thought
/// under the header, past the show delay.
fn thinking_block() -> App {
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking("one\ntwo\nthree");
    app.set_status_times(Duration::ZERO, Some(REASONING_SHOW_DELAY));
    app
}

/// Settle the open phase the way the boundary does: record it and stop the
/// clock. Its `Thought for …` line and spacer are the two rows it commits.
fn settle_thought(app: &mut App) -> usize {
    app.finish_reasoning(1).expect("the phase had text");
    app.set_status_times(Duration::ZERO, None);
    2
}

/// A turn whose `bash` call is running with `lines` of output streamed — the
/// live tail with its clock row under it.
fn running_command(lines: usize) -> App {
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "python3 app.py", None);
    app.set_command_elapsed(Some(Duration::from_secs(2)));
    let output: String = (1..=lines).map(|n| format!("line {n}\n")).collect();
    app.push_tool_output(&output);
    app
}

/// Resolve the running call the way `commit_tool_cell` does, returning the
/// rows it commits: its settled cell and a spacer.
fn resolve_command(app: &mut App, lines: usize) -> usize {
    let output: Vec<String> = (1..=lines).map(|n| format!("line {n}")).collect();
    app.end_tool(&output.join("\n"), true)
        .expect("a call was running");
    app.set_command_elapsed(None);
    let _ = app.place_resolved_tool();
    let cell = tool_commit_lines(&app.history, app.tool_queue(), W, app.path_display())
        .expect("the call commits its cell");
    cell.len() + 1
}

#[test]
fn a_settled_thought_holds_its_rows_until_the_call_lands() {
    // The report: the block settles at the round's first tool-call piece, but
    // the call has a cell only once its batch is announced — so the box rose
    // by the block's rows and dropped back when the cell landed.
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    let committed = settle_thought(&mut app);
    assert_eq!(
        screen.paint(&mut app, committed),
        0,
        "the box holds still as the thought settles"
    );
    assert_eq!(
        app.strip_hold(),
        3,
        "the block's five rows, less the two its settle committed"
    );
    screen.wait(ms(80));
    app.start_tool_batch(&[summary("Skill", "web-search")]);
    assert_eq!(
        screen.paint(&mut app, 0),
        0,
        "the call's cell lands in the held rows"
    );
    assert_eq!(app.strip_hold(), 0, "the cell took the rows back");
}

#[test]
fn a_settled_thought_gives_way_to_the_replys_first_words_without_a_hop() {
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    let committed = settle_thought(&mut app);
    assert_eq!(screen.paint(&mut app, committed), 0);
    app.push_chunk("The answer");
    app.set_stream_preview_rows(1);
    assert_eq!(
        screen.paint(&mut app, 0),
        0,
        "the reply's frontier lands where the block stood"
    );
    assert_eq!(app.strip_hold(), 1, "one row still owed under the frontier");
    // The frontier's first line commits: that row is what was owed.
    assert_eq!(screen.paint(&mut app, 1), 0);
    assert_eq!(app.strip_hold(), 0, "the commit used the hold up");
}

#[test]
fn a_commands_tail_settling_holds_exactly_the_row_it_owes() {
    // A running command carries a clock row its settled peek does not, so
    // the round's last command lifted the box a row until the next round
    // answered — and one row is exactly what is held, not a preview slot's
    // two (the floor's mistake).
    let mut app = running_command(6);
    let mut screen = Screen::new(&mut app, H);
    let committed = resolve_command(&mut app, 6);
    assert_eq!(
        screen.paint(&mut app, committed),
        0,
        "the box holds still as the tail settles"
    );
    assert_eq!(app.strip_hold(), 1, "exactly the clock row's");
    // The next round opens on its reply's first words: they take the row.
    screen.wait(ms(400));
    app.push_chunk("Next");
    app.set_stream_preview_rows(1);
    assert!(
        screen.paint(&mut app, 0) >= 0,
        "the next round's content never lifts the box"
    );
    assert_eq!(app.strip_hold(), 0);
}

#[test]
fn a_hold_nothing_takes_lapses_after_the_strip_hold() {
    // A hold never lingers through a slow request: what the floor got wrong.
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    let committed = settle_thought(&mut app);
    assert_eq!(screen.paint(&mut app, committed), 0);
    screen.wait(STRIP_HOLD - ms(1));
    assert_eq!(screen.paint(&mut app, 0), 0, "still held just short of it");
    assert_eq!(app.strip_hold(), 3);
    screen.wait(ms(1));
    assert_eq!(
        screen.paint(&mut app, 0),
        -3,
        "the box settles once when the hold lapses"
    );
    assert_eq!(app.strip_hold(), 0);
    // A hand-off after the lapse is held afresh — the old clock is gone.
    app.start_tool_batch(&[summary("Bash", "python3 app.py")]);
    screen.paint(&mut app, 0);
    app.start_tool("Bash", "python3 app.py", None);
    app.set_command_elapsed(Some(Duration::from_secs(1)));
    app.push_tool_output("a\nb\nc\nd\ne\nf\n");
    screen.paint(&mut app, 0);
    screen.wait(ms(10));
    let committed = resolve_command(&mut app, 6);
    assert_eq!(screen.paint(&mut app, committed), 0);
    assert_eq!(app.strip_hold(), 1, "a fresh hold, on its own clock");
}

#[test]
fn a_lapsed_hold_leaves_no_clock_behind() {
    // The tracker's own contract, frame by frame: rows owed after a lapse are
    // a new hold on a new clock, even when they are owed in the very next
    // frame. (Played with the reply's flush left uncommitted, so its rows are
    // owed at once.)
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    let committed = settle_thought(&mut app);
    screen.paint(&mut app, committed);
    app.push_chunk("The answer");
    app.set_stream_preview_rows(1);
    screen.paint(&mut app, 0);
    assert_eq!(app.strip_hold(), 1, "the frontier fills all but a row");
    screen.wait(STRIP_HOLD);
    screen.paint(&mut app, 0);
    assert_eq!(app.strip_hold(), 0, "that row's hold lapsed");
    let _ = app.flush_streaming_segment();
    screen.wait(ms(1));
    screen.paint(&mut app, 0);
    assert_eq!(app.strip_hold(), 2, "the frontier's rows, held afresh");
}

#[test]
fn an_open_thinking_phase_keeps_the_hold_from_lapsing() {
    // A phase previews nothing for its first second, and the line it settles
    // into commits the rows the hold kept: letting the hold lapse during that
    // second only for the line to push the box straight back down was the
    // bounce this exists to remove.
    let mut app = running_command(6);
    let mut screen = Screen::new(&mut app, H);
    let committed = resolve_command(&mut app, 6);
    assert_eq!(screen.paint(&mut app, committed), 0);
    assert_eq!(app.strip_hold(), 1);
    // The next round opens on a thought just inside the hold…
    screen.wait(STRIP_HOLD - ms(100));
    app.begin_reasoning();
    app.push_thinking("next step");
    app.set_status_times(Duration::ZERO, Some(ms(10)));
    assert_eq!(screen.paint(&mut app, 0), 0);
    // …and is still thinking, unseen, well past it.
    screen.wait(ms(600));
    app.set_status_times(Duration::ZERO, Some(ms(610)));
    assert_eq!(screen.paint(&mut app, 0), 0, "no lapse while it thinks");
    assert_eq!(app.strip_hold(), 1);
    // Its line commits two rows: the held one and one more.
    let committed = settle_thought(&mut app);
    assert_eq!(
        screen.paint(&mut app, committed),
        1,
        "the thought's line moves the box down a row, like any commit"
    );
    assert_eq!(app.strip_hold(), 0);
}

#[test]
fn commits_that_cover_the_rows_hold_nothing() {
    // A cell committing more than it stood on owes nothing: no padding, the
    // box simply moves down.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "python3 app.py", None);
    let mut screen = Screen::new(&mut app, H);
    let committed = resolve_command(&mut app, 3);
    assert!(screen.paint(&mut app, committed) > 0);
    assert_eq!(app.strip_hold(), 0);
}

#[test]
fn the_hold_never_squeezes_the_composer() {
    // The rows the composer path leaves are all a hold may take: follow-ups
    // queued as the thought settles fill a short terminal, so of the three
    // rows owed only the ones the terminal still has are held.
    let term = 14;
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, term);
    for text in ["first", "second", "third", "fourth"] {
        app.queued.push_back(batch(&[text]));
    }
    let committed = settle_thought(&mut app);
    screen.paint(&mut app, committed);
    let held = app.strip_hold();
    assert!(
        held > 0 && held < 3,
        "clamped short of the three owed: {held}"
    );
    let rows = live_rows(&app, W, term);
    let asked = strip_rows(rows.has_status, rows.preview, rows.hang)
        + rows.queued
        + rows.toast
        + INPUT_CHROME_ROWS
        + 1
        + rows.band
        + rows.footer
        + rows.agent;
    assert_eq!(
        asked, term,
        "the hold takes exactly the rows the terminal has left"
    );
    assert_eq!(live_height_for(&app.input, W, term, &rows), asked);
}

#[test]
fn a_frame_with_no_live_strip_owes_nothing_after_it() {
    // Under a picker the region is the picker's: closing it starts the hold
    // over rather than holding rows the composer path never painted.
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    app.open_settings();
    screen.paint(&mut app, 0);
    app.settings_picker = None;
    let committed = settle_thought(&mut app);
    screen.paint(&mut app, committed);
    assert_eq!(app.strip_hold(), 0);
}

#[test]
fn nothing_is_held_outside_a_turn() {
    // A value injected a frame ago must never pad an idle composer.
    let mut app = App::new();
    app.set_strip_hold(4);
    assert_eq!(hang_rows(&app, W), 0);
    let mut hold = StripHold::default();
    assert_eq!(hold.rows(&app, 0, W, H, Duration::ZERO), 0);
}

#[test]
fn nothing_is_held_under_a_modal_prompt() {
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    settle_thought(&mut app);
    app.open_permission(PermissionRequest {
        id: "perm_0".to_string(),
        kind: PermissionKind::Bash,
        target: "rm -rf build".to_string(),
        body: String::new(),
        detail: None,
        agent: None,
        agent_id: None,
    });
    assert_eq!(screen.hold.rows(&app, 2, W, H, screen.now), 0);
    app.set_strip_hold(3);
    assert_eq!(hang_rows(&app, W), 0, "the modal replaces the strip");
}

#[test]
fn nothing_is_held_under_a_composer_replacing_view() {
    // A picker lays the strip out by its own split; the hold is the composer
    // path's.
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    settle_thought(&mut app);
    app.open_settings();
    assert_eq!(screen.hold.rows(&app, 2, W, H, screen.now), 0);
}

#[test]
fn nothing_is_held_in_a_shell_turn() {
    // A `!` command's turn shows no status line for rows to sit above.
    let mut app = App::new();
    app.begin_shell("ls");
    let mut hold = StripHold::default();
    hold.painted(&app, W, H);
    assert_eq!(hold.rows(&app, 0, W, H, Duration::ZERO), 0);
}

#[test]
fn a_rebuild_starts_the_hold_over() {
    // A purge rebuild re-seats the region: nothing it painted before is
    // owed any more.
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    settle_thought(&mut app);
    screen.hold.reset();
    assert_eq!(screen.hold.rows(&app, 2, W, H, screen.now), 0);
}

#[test]
fn the_held_rows_are_blank_and_sit_directly_above_the_status_line() {
    let mut app = thinking_block();
    let mut screen = Screen::new(&mut app, H);
    let committed = settle_thought(&mut app);
    screen.paint(&mut app, committed);
    assert_eq!(app.strip_hold(), 3);
    let height = region(&app, H);
    let mut buf = buffer(W, height);
    render_live(buf.area, &mut buf, &app);
    let status = (0..height)
        .find(|&y| row(&buf, y, W).contains("esc to interrupt"))
        .expect("the status line is drawn");
    assert_eq!(status, 3, "three held rows over the status line");
    for y in 0..status {
        assert!(
            row(&buf, y, W).trim().is_empty(),
            "held row {y} is blank: {:?}",
            row(&buf, y, W)
        );
    }
}

#[test]
fn the_held_rows_never_flow_into_scrollback() {
    // Held rows are geometry, not content: a terminal short enough to make
    // the strip flow must not flow blank rows (docs/strip-flow.md).
    for term in 8..20 {
        let mut app = thinking_block();
        let mut screen = Screen::new(&mut app, term);
        let committed = settle_thought(&mut app);
        screen.paint(&mut app, committed);
        assert!(
            view_flow(&app, W, term, 10_000).is_none(),
            "nothing flows at {term} rows (held {})",
            app.strip_hold()
        );
    }
}

#[test]
fn a_subagents_strip_holds_its_thought_the_same_way() {
    // The agent session view's strip works the main one's way, so its
    // hand-offs hopped the same way (docs/agent-view-streaming.md).
    let mut app = App::new();
    app.begin_stream();
    app.start_agent_group(false, &[spec("a1", "Think", false)]);
    app.begin_agent_reasoning("a1");
    app.apply_agent_event("a1", &StreamEvent::ThinkingChunk("one\ntwo\nthree".into()));
    app.set_agent_thinking("a1", REASONING_SHOW_DELAY);
    app.open_agent_view("a1");
    let mut screen = Screen::new(&mut app, H);
    // The boundary's order (`Session::settle_agent_reasoning`): settle the
    // phase, then fold the event.
    app.finish_agent_reasoning("a1", 1)
        .expect("the agent's phase had text");
    app.apply_agent_event("a1", &StreamEvent::ThinkingEnd);
    app.clear_agent_thinking();
    assert_eq!(screen.paint(&mut app, 2), 0, "the agent's box holds still");
    assert_eq!(app.strip_hold(), 3);
}
