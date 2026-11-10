//! Settling cells: a finished live cell keeps the rows it stood on until what
//! comes next takes them, so the box never jumps up between one cell and the
//! next. See `docs/settling.md`.

use super::*;
use crate::app::{Settling, SettlingBody, SettlingDress, SettlingHead};
use crate::ui::live::{preview_lines, strip_lines};
use crate::ui::settling::{
    SettleGeometry, command_picture, group_picture, settle_frame, settle_plan, settles_in_place,
    settling_cell, settling_holds, settling_lines, thought_picture,
};
use crate::ui::theme::{
    REASONING_PEEK_LINES, REASONING_TEXT_MODIFIER, TOOL_BULLET, TOOL_PEEK_ROWS,
    reasoning_text_color, tool_fail_color, tool_ok_color,
};

const WIDTH: u16 = 80;

/// The frame clock most tests draw at: a moment at which a running bullet
/// shows (`docs/tool-pulse.md`).
const PULSE: Duration = Duration::ZERO;

/// The rows the strip's preview slot shows right now, as text.
fn strip(app: &App, width: u16) -> Vec<String> {
    preview_lines(app, width, None, preview_rows(app, width))
        .iter()
        .map(plain)
        .collect()
}

fn texts(lines: &[Line<'_>]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

/// A turn in which the model is thinking `lines` lines of thought.
fn thinking(lines: usize) -> App {
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    for line in 1..=lines {
        app.push_thinking(&format!("thought number {line}\n"));
    }
    app
}

/// A turn running `make test`, which has printed `lines` lines so far.
fn running(lines: usize) -> App {
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "make test", None);
    for line in 1..=lines {
        app.push_tool_output(&format!("output {line}\n"));
    }
    app.set_command_elapsed(Some(Duration::from_secs(2)));
    app
}

/// End the open thought the way the boundary does: take its picture, record
/// it, and build the cell over the line it commits.
fn settle_thought(app: &mut App, width: u16) -> (Settling, Vec<Line<'static>>) {
    let picture = thought_picture(app, width, 2);
    let reasoning = app.finish_reasoning(2).expect("the phase had text");
    let committed = reasoning_lines(&reasoning, width);
    (settling_cell(picture, &committed), committed)
}

/// End the running command with `output`, the boundary's way.
fn settle_command(
    app: &mut App,
    output: &str,
    ok: bool,
    width: u16,
) -> (Settling, Vec<Line<'static>>) {
    let picture = command_picture(app, ok, width);
    app.end_tool(output, ok).expect("a call was running");
    let committed = tool_commit_lines(&app.history, app.tool_queue(), width, app.path_display())
        .expect("the call commits a cell of its own");
    (settling_cell(picture, &committed), committed)
}

// ----- the pictures: the rows the live cell stood on --------------------

#[test]
fn a_thought_that_ended_keeps_its_block_exactly_as_it_was() {
    // The whole point: the frame a phase ends, the strip draws what it drew
    // the frame before — the same header, still blinking and shimmering,
    // over the same rows of thought — so nothing on screen changes until
    // the next cell lands. The `Thought for …` line is what the cell
    // commits, not what it wears meanwhile.
    for lines in 1..=6 {
        for pulse in [0, 300, 650, 1234] {
            let mut app = thinking(lines);
            app.set_pulse(Duration::from_millis(pulse));
            let live = preview_lines(&app, WIDTH, None, preview_rows(&app, WIDTH));
            assert_eq!(
                live.len(),
                1 + lines.min(REASONING_PEEK_LINES),
                "the block: its header over the tail"
            );
            let (cell, committed) = settle_thought(&mut app, WIDTH);
            assert_eq!(cell.full_rows(), live.len(), "{lines} lines of thought");
            assert_eq!(cell.settled_rows(), 1, "one line commits");
            assert!(plain(&committed[0]).starts_with("Thought for 2s"));
            assert_eq!(
                settling_lines(&cell, app.pulse()),
                live,
                "{lines} lines at {pulse} ms: text and style alike"
            );
        }
    }
}

#[test]
fn the_settled_line_is_never_drawn_over_its_own_thought() {
    // Tried, and it read as a flash of stale thought: a line saying the
    // thought is over, with the thought still under it. While any row of
    // thought is on screen the header is the live one; the line shows only
    // once the cell is down to it — and then it is the line, exactly.
    let mut app = thinking(3);
    let (mut cell, committed) = settle_thought(&mut app, WIDTH);
    for rows in (2..=cell.full_rows()).rev() {
        cell.keep(rows);
        let drawn = texts(&settling_lines(&cell, PULSE));
        assert_eq!(drawn.len(), rows);
        assert_eq!(drawn[0], "● Thinking…", "{rows} rows: {drawn:?}");
        assert!(
            !drawn.iter().any(|row| row.starts_with("Thought for")),
            "{rows} rows: {drawn:?}"
        );
    }
    cell.keep(1);
    assert_eq!(settling_lines(&cell, PULSE), committed);
}

#[test]
fn a_thought_that_gave_a_row_up_stands_at_rest() {
    // Whole, the block is indistinguishable from an open phase's: the model
    // is writing its call and nothing has changed. Once something has
    // landed and taken a row — the reply's first words — the thought is
    // visibly over, and a header still blinking beside a streaming answer
    // would say otherwise: it stands at rest until the rest goes.
    let hidden = Duration::from_millis(650); // the half-second a live bullet hides
    let mut app = thinking(3);
    let (mut cell, _) = settle_thought(&mut app, WIDTH);
    let whole = settling_lines(&cell, hidden);
    assert_eq!(plain(&whole[0]), "  Thinking…", "whole: still blinking");
    cell.keep(cell.full_rows() - 1);
    let shrunk = settling_lines(&cell, hidden);
    assert_eq!(
        plain(&shrunk[0]),
        "● Thinking…",
        "at rest: the bullet shows"
    );
    assert_eq!(
        shrunk[0],
        settling_lines(&cell, Duration::ZERO)[0],
        "and nothing about it moves with the clock"
    );
    assert_eq!(shrunk[0].spans.len(), 2, "a flat label: no shimmer");
}

#[test]
fn a_group_that_gave_a_row_up_stands_at_rest() {
    let hidden = Duration::from_millis(650);
    let mut app = App::new();
    app.begin_stream();
    let path = format!("src/{}/mod.rs", "deeply/nested".repeat(8));
    app.start_tool("Read", &path, None);
    app.end_tool("1 x", true);
    assert_eq!(app.place_resolved_tool(), crate::app::ToolPlacement::Held);
    let picture = group_picture(&app, 60);
    let range = app.close_tool_group().expect("a group was open");
    let committed = tool_group_lines(&app.history[range], 60, app.path_display());
    let mut cell = settling_cell(picture, &committed);
    assert!(cell.full_rows() >= 3, "the path wraps under the corner");
    assert!(plain(&settling_lines(&cell, hidden)[0]).starts_with("  Reading"));
    cell.keep(cell.full_rows() - 1);
    assert!(plain(&settling_lines(&cell, hidden)[0]).starts_with("● Reading"));
}

#[test]
fn a_thought_whose_block_still_stands_keeps_its_clause_on_the_status_line() {
    // The block's other half: `Thinking for 2s` stays on the status line
    // beside it, so the phase's end changes nothing on screen. It goes when
    // the block does.
    let status = |app: &App| {
        let rows = preview_rows(app, WIDTH);
        strip_lines(app, WIDTH, None, rows)
            .iter()
            .map(plain)
            .find(|row| row.contains("esc to interrupt"))
            .expect("a turn is in flight")
    };
    let mut app = thinking(3);
    app.set_status_times(Duration::from_secs(5), Some(Duration::from_secs(2)));
    assert!(
        status(&app).contains("Thinking for 2s"),
        "the phase is open"
    );
    let (cell, _) = settle_thought(&mut app, WIDTH);
    app.set_status_times(Duration::from_secs(5), None);
    assert!(
        !status(&app).contains("Thinking for"),
        "no block, no clause"
    );
    app.push_settling(cell);
    assert!(
        status(&app).contains("Thinking for 2s"),
        "the block still stands: {}",
        status(&app)
    );
    // A new phase's own clock outranks it.
    app.set_status_times(Duration::from_secs(9), Some(Duration::from_secs(1)));
    assert!(status(&app).contains("Thinking for 1s"));
    app.set_status_times(Duration::from_secs(9), None);
    assert!(status(&app).contains("Thinking for 2s"));
    // The reply's first words took a row: the thought is visibly over.
    app.keep_settling(0, 3);
    assert!(
        !status(&app).contains("Thinking for"),
        "a block that gave a row up is no longer thinking: {}",
        status(&app)
    );
}

#[test]
fn a_settling_thought_keeps_its_rows_dim_and_italic() {
    // The rows read the same the frame the phase stops: the thought's dress.
    let mut app = thinking(2);
    let (cell, _) = settle_thought(&mut app, WIDTH);
    let lines = settling_lines(&cell, PULSE);
    let body = lines[1].spans.last().expect("the row has text");
    assert_eq!(body.style.fg, Some(reasoning_text_color()));
    assert!(body.style.add_modifier.contains(REASONING_TEXT_MODIFIER));
}

#[test]
fn a_command_settles_on_exactly_the_rows_its_running_cell_stood_on() {
    // The running cell is one row taller than the cell it commits — its
    // clock row — and the box used to jump by that row the moment the
    // command ended. Whatever it printed, and whether or not the Ctrl+B hint
    // was showing, the cell it leaves stands on the same rows.
    for lines in [1usize, 2, 3, 4, 5, 9, 40] {
        for seconds in [1u64, 30] {
            let mut app = running(lines);
            app.set_command_elapsed(Some(Duration::from_secs(seconds)));
            let live = strip(&app, WIDTH);
            assert_eq!(
                live.len(),
                1 + lines.min(TOOL_PEEK_ROWS) + 1,
                "header, the tail window, the clock row"
            );
            let output: String = (1..=lines).map(|n| format!("output {n}\n")).collect();
            let (cell, committed) =
                settle_command(&mut app, &format!("Exit code: 0\n{output}"), true, WIDTH);
            assert_eq!(
                cell.full_rows(),
                live.len(),
                "{lines} lines after {seconds}s"
            );
            assert_eq!(cell.settled_rows(), committed.len());
            let drawn = texts(&settling_lines(&cell, PULSE));
            let last = drawn.len() - 1;
            assert_eq!(drawn[0], live[0], "the header stays where it was");
            assert_eq!(
                drawn[1..last],
                live[1..last],
                "the tail it was showing stays under it"
            );
            let hidden = lines.saturating_sub(TOOL_PEEK_ROWS);
            let clock = format!("({seconds}s)");
            let footer = if hidden > 0 {
                format!("     +{hidden} lines {clock}")
            } else {
                format!("     {clock}")
            };
            assert_eq!(
                drawn[last], footer,
                "the clock stops: no wait left to count, no key left to hint"
            );
        }
    }
}

#[test]
fn a_settling_command_wears_its_outcome_on_the_bullet() {
    // A finished cell must not read as a running one: the bullet takes the
    // outcome's colour the frame the command ends, and stops blinking.
    for (ok, color) in [(true, tool_ok_color()), (false, tool_fail_color())] {
        let mut app = running(3);
        let (cell, _) = settle_command(&mut app, "Exit code: 1\nboom\n", ok, WIDTH);
        let bullet = &settling_lines(&cell, PULSE)[0].spans[0];
        assert_eq!(bullet.content, TOOL_BULLET);
        assert_eq!(bullet.style.fg, Some(color));
    }
}

#[test]
fn a_command_that_printed_nothing_leaves_no_picture() {
    // `⎿ Running… (2s · wait 2m)` settles into `⎿ (no output)`: one row for
    // one row, nothing to hold.
    let app = running(0);
    assert!(command_picture(&app, true, WIDTH).is_none());
}

#[test]
fn only_a_running_command_with_a_cell_of_its_own_leaves_a_picture() {
    // A read's cell commits taller than it ran — nothing to hold.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Read", "about.py", None);
    assert!(
        command_picture(&app, true, WIDTH).is_none(),
        "not a command"
    );

    // A `!` command ends its own turn: its cell is the turn's last word.
    let mut app = App::new();
    app.begin_shell("seq 1 9");
    app.push_tool_output("1\n2\n");
    assert!(
        command_picture(&app, true, WIDTH).is_none(),
        "a `!` command"
    );

    // A batch sibling that has not started has nothing under its header.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool_batch(&[crate::stream::ToolCallSummary {
        name: "Bash".to_string(),
        args: "make".to_string(),
    }]);
    assert!(
        command_picture(&app, true, WIDTH).is_none(),
        "still waiting"
    );

    // Nothing in flight at all.
    let mut app = App::new();
    app.begin_stream();
    assert!(command_picture(&app, true, WIDTH).is_none());
}

#[test]
fn a_command_folded_into_the_read_group_leaves_no_picture() {
    // With Verbose output off a listing is part of the collapsed group's
    // cell, which stays open when the call ends — nothing settles.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "ls -la", None);
    app.push_tool_output("total 4\n");
    assert!(!app.settings().verbose);
    assert!(command_picture(&app, true, WIDTH).is_none());

    // …and has a cell of its own once it is asked for.
    app.settings_mut().verbose = true;
    assert!(command_picture(&app, true, WIDTH).is_some());
}

#[test]
fn only_the_block_the_strip_is_showing_leaves_a_picture() {
    // No phase open.
    let mut app = App::new();
    app.begin_stream();
    assert!(thought_picture(&app, WIDTH, 0).is_none());

    // A phase with no text yet has only its header: nothing under it.
    app.begin_reasoning();
    assert!(thought_picture(&app, WIDTH, 0).is_none());

    // A thought inside an open read/search group shows no block at all — the
    // group's cell holds the strip (docs/tool-groups.md *Thoughts*).
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Read", "about.py", None);
    app.end_tool("1 x", true);
    assert_eq!(app.place_resolved_tool(), crate::app::ToolPlacement::Held);
    app.begin_reasoning();
    app.push_thinking("which file next\n");
    assert!(thought_picture(&app, WIDTH, 0).is_none());
}

#[test]
fn a_group_that_closed_keeps_its_cell_exactly_as_it_was() {
    // `● Reading 1 file… (ctrl+o to expand)` over `⎿ about.py` commits as
    // the one line `Read 1 file (ctrl+o to expand)`: a row short. Until that
    // row is taken the cell stays as it was, its bullet still blinking — the
    // settled line is drawn alone, never over the value it stood for.
    for pulse in [0, 300, 650, 1234] {
        let mut app = App::new();
        app.begin_stream();
        app.start_tool("Read", "about.py", None);
        app.end_tool("1 x", true);
        assert_eq!(app.place_resolved_tool(), crate::app::ToolPlacement::Held);
        app.set_pulse(Duration::from_millis(pulse));
        let live = preview_lines(&app, WIDTH, None, preview_rows(&app, WIDTH));
        assert_eq!(live.len(), 2, "{live:?}");

        let picture = group_picture(&app, WIDTH);
        let range = app.close_tool_group().expect("a group was open");
        let committed = tool_group_lines(&app.history[range], WIDTH, app.path_display());
        assert_eq!(texts(&committed), ["Read 1 file (ctrl+o to expand)"]);
        let mut cell = settling_cell(picture, &committed);
        assert_eq!(cell.full_rows(), live.len());
        assert_eq!(cell.settled_rows(), 1);
        assert_eq!(
            settling_lines(&cell, app.pulse()),
            live,
            "at {pulse} ms: text and style alike"
        );
        cell.keep(1);
        assert_eq!(settling_lines(&cell, app.pulse()), committed);
    }
}

#[test]
fn no_group_open_leaves_no_group_picture() {
    let mut app = App::new();
    app.begin_stream();
    assert!(group_picture(&app, WIDTH).is_none());
}

#[test]
fn a_cell_whose_committed_form_is_no_shorter_draws_that_form() {
    // A command whose final output outran what it streamed commits taller
    // than it ran. If it has to wait its turn behind an older cell it is
    // drawn as what it commits — never as a tail it has already outgrown.
    let mut app = running(1);
    let output = "Exit code: 0\na\nb\nc\nd\n";
    let (cell, committed) = settle_command(&mut app, output, true, WIDTH);
    assert!(committed.len() > 3, "{committed:?}");
    assert!(cell.is_settled());
    assert_eq!(settling_lines(&cell, PULSE), committed);
}

// ----- giving rows up ----------------------------------------------------

#[test]
fn a_command_gives_up_its_oldest_rows_first_and_counts_them() {
    // The window shrinks the way it scrolled: from the top. The row left on
    // top takes the corner, and the footer counts what went.
    let mut app = running(9);
    let (mut cell, _) = settle_command(
        &mut app,
        "Exit code: 0\nonly\n\nthe rest\nand more\nand more\nand more\n",
        true,
        WIDTH,
    );
    assert_eq!(
        texts(&settling_lines(&cell, PULSE)),
        [
            "● Bash(make test)",
            "  ⎿  output 6",
            "     output 7",
            "     output 8",
            "     output 9",
            "     +5 lines (2s)",
        ]
    );
    cell.keep(cell.rows() - 2);
    assert_eq!(
        texts(&settling_lines(&cell, PULSE)),
        [
            "● Bash(make test)",
            "  ⎿  output 8",
            "     output 9",
            "     +7 lines (2s)",
        ]
    );
}

#[test]
fn a_command_whose_output_fitted_starts_counting_when_it_gives_a_row_up() {
    let mut app = running(3);
    let (mut cell, _) = settle_command(&mut app, "Exit code: 0\nx\n", true, WIDTH);
    assert_eq!(
        texts(&settling_lines(&cell, PULSE)).last().unwrap(),
        "     (2s)"
    );
    cell.keep(cell.rows() - 1);
    assert_eq!(
        texts(&settling_lines(&cell, PULSE)),
        [
            "● Bash(make test)",
            "  ⎿  output 2",
            "     output 3",
            "     +1 lines (2s)",
        ]
    );
}

#[test]
fn a_thought_gives_up_its_oldest_rows_first() {
    let mut app = thinking(3);
    let (mut cell, committed) = settle_thought(&mut app, WIDTH);
    cell.keep(2);
    assert_eq!(
        texts(&settling_lines(&cell, PULSE)),
        ["● Thinking…", "  ⎿  thought number 3"],
        "the newest row is the last to go, and wears the corner"
    );
    assert!(plain(&committed[0]).starts_with("Thought for 2s"));
}

#[test]
fn a_group_gives_up_its_hint_from_the_bottom() {
    // A path too long for one row wraps under the corner: the corner row is
    // the last to go.
    let mut app = App::new();
    app.begin_stream();
    let path = format!("src/{}/mod.rs", "deeply/nested".repeat(8));
    app.start_tool("Read", &path, None);
    app.end_tool("1 x", true);
    assert_eq!(app.place_resolved_tool(), crate::app::ToolPlacement::Held);
    let live = strip(&app, 60);
    assert!(live.len() >= 3, "the path wraps: {live:?}");
    let picture = group_picture(&app, 60);
    let range = app.close_tool_group().expect("a group was open");
    let committed = tool_group_lines(&app.history[range], 60, app.path_display());
    let mut cell = settling_cell(picture, &committed);
    cell.keep(2);
    assert_eq!(texts(&settling_lines(&cell, PULSE))[1], live[1]);
}

#[test]
fn a_cell_down_to_its_committed_height_is_what_it_commits() {
    // Waiting its turn behind an older cell, it already looks the way it
    // will in scrollback: the peek from the top over its hint, not a tail
    // cut short.
    let mut app = running(9);
    let output = "Exit code: 0\nfirst\n\nsecond\nthird\nfourth\nfifth\n";
    let (mut cell, committed) = settle_command(&mut app, output, true, WIDTH);
    assert!(!cell.is_settled());
    assert_ne!(settling_lines(&cell, PULSE), committed);
    cell.keep(cell.settled_rows());
    assert!(cell.is_settled());
    assert_eq!(settling_lines(&cell, PULSE), committed);
}

#[test]
fn a_cell_draws_exactly_the_rows_it_keeps() {
    let mut app = running(9);
    let (mut cell, _) = settle_command(&mut app, "Exit code: 0\nx\n", true, WIDTH);
    for rows in (cell.settled_rows()..=cell.full_rows()).rev() {
        cell.keep(rows);
        assert_eq!(settling_lines(&cell, PULSE).len(), cell.rows());
    }
}

// ----- in the strip ------------------------------------------------------

#[test]
fn the_strip_draws_a_settling_cell_where_its_live_cell_was() {
    // Between rounds: the command has ended, the next request is out. The
    // strip is the cell it left, then the gap, then the status line — the
    // rows it had a frame ago.
    let mut app = running(9);
    let before = strip(&app, WIDTH);
    let (cell, _) = settle_command(&mut app, "Exit code: 0\nx\n", true, WIDTH);
    app.push_settling(cell);
    let after = strip(&app, WIDTH);
    assert_eq!(after.len(), before.len(), "{after:?}");
    assert_eq!(usize::from(preview_rows(&app, WIDTH)), after.len());
    assert_eq!(after[0], "● Bash(make test)");
}

#[test]
fn settling_cells_stand_above_what_is_live_one_blank_row_apart() {
    // A thought ended, its call's cell landed, and the frame has not let the
    // thought go yet: oldest on top, a blank row between cells — where each
    // will sit in scrollback.
    let mut app = thinking(3);
    let (thought, _) = settle_thought(&mut app, WIDTH);
    app.push_settling(thought);
    app.start_tool("Skill", "web-search", None);
    assert_eq!(
        strip(&app, WIDTH),
        [
            "● Thinking…",
            "  ⎿  thought number 1",
            "     thought number 2",
            "     thought number 3",
            "",
            "● Skill(web-search)",
            "  ⎿  Running…",
        ]
    );
    assert_eq!(usize::from(preview_rows(&app, WIDTH)), 7);
}

#[test]
fn two_settling_cells_are_one_blank_row_apart() {
    let mut app = App::new();
    app.begin_stream();
    app.push_settling(Settling::settled(vec![Line::from("first")]));
    app.push_settling(Settling::settled(vec![Line::from("second")]));
    assert_eq!(strip(&app, WIDTH), ["first", "", "second"]);
    assert_eq!(preview_rows(&app, WIDTH), 3);
}

#[test]
fn the_rows_reserved_and_the_rows_drawn_agree_with_cells_settling() {
    // `preview_rows` sizes the region and `preview_lines` paints it: one
    // walk, whatever else is in the strip.
    let cell = || Settling::settled(vec![Line::from("a settled cell"), Line::from("  ⎿  row")]);
    // Over a reply's frontier.
    let mut app = App::new();
    app.begin_stream();
    app.push_chunk("the answer starts");
    app.push_settling(cell());
    assert_eq!(
        usize::from(preview_rows(&app, WIDTH)),
        strip(&app, WIDTH).len()
    );
    assert_eq!(preview_rows(&app, WIDTH), 2 + 1 + 1);
    // Over a new thinking block.
    let mut app = thinking(2);
    app.push_settling(cell());
    assert_eq!(
        usize::from(preview_rows(&app, WIDTH)),
        strip(&app, WIDTH).len()
    );
    assert_eq!(preview_rows(&app, WIDTH), 2 + 1 + 3);
    // Over a running command.
    let mut app = running(2);
    app.push_settling(cell());
    assert_eq!(
        usize::from(preview_rows(&app, WIDTH)),
        strip(&app, WIDTH).len()
    );
    // Alone, in the pre-stream pause of the next round.
    let mut app = App::new();
    app.begin_stream();
    app.push_settling(cell());
    assert_eq!(preview_rows(&app, WIDTH), 2);
    assert_eq!(strip(&app, WIDTH).len(), 2);
}

#[test]
fn a_squeezed_slot_draws_exactly_the_rows_it_was_given() {
    // The geometry and the paint must agree at every size the region can be
    // squeezed to, or the box and the cursor unseat (`render_live`'s
    // assertion). The boundary lets the cells go before it comes to that,
    // but the renderer holds the contract by itself.
    let mut app = running(9);
    app.push_settling(Settling::settled(vec![
        Line::from("a settled cell"),
        Line::from("  ⎿  row"),
    ]));
    let want = preview_rows(&app, WIDTH);
    for rows in 0..=want {
        assert_eq!(
            preview_lines(&app, WIDTH, None, rows).len(),
            usize::from(rows),
            "given {rows} of {want} rows"
        );
    }
}

#[test]
fn the_live_region_paints_with_a_cell_settling() {
    // End to end through `render_live`: the cell, the gap, the status line.
    let mut app = running(9);
    let (cell, _) = settle_command(&mut app, "Exit code: 0\nx\n", true, 60);
    app.push_settling(cell);
    let band = band_rows(&app, 60);
    let height = live_height(
        &app.input,
        60,
        24,
        strip_has_status(&app),
        preview_rows(&app, 60),
        0,
        0,
        0,
        band,
        footer_rows(&app, band),
        0,
    );
    let mut buf = buffer(60, height);
    render_live(buf.area, &mut buf, &app);
    assert_eq!(row(&buf, 0, 60).trim_end(), "● Bash(make test)");
    assert_eq!(row(&buf, 5, 60).trim_end(), "     +5 lines (2s)");
    assert!(row(&buf, 6, 60).trim().is_empty(), "one blank row");
    assert!(
        row(&buf, 7, 60).contains("esc to interrupt"),
        "then the status line: {:?}",
        row(&buf, 7, 60)
    );
}

// ----- the plan: how many rows each cell keeps ---------------------------

/// A cell `rows` tall that commits as `settled` rows.
fn cell(rows: usize, settled: usize) -> Settling {
    let body: Vec<String> = (1..rows).map(|row| format!("row {row}")).collect();
    Settling::new(
        SettlingHead::Lines(vec![Line::from("head")]),
        SettlingBody::Tail {
            rows: body,
            dress: SettlingDress::Output,
            hidden: 0,
            clock: None,
        },
        (0..settled)
            .map(|row| Line::from(format!("committed {row}")))
            .collect(),
    )
}

/// The plan for a region flush with the bottom of a 30-row screen: `height`
/// rows tall with the cells as they are, `pending` rows queued above it.
fn plan_at_the_bottom(height: u16, pending: usize, cells: &[Settling]) -> Vec<usize> {
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 30 - height,
        pending_rows: pending,
        height,
        holds: true,
    };
    settle_plan(&geometry, cells)
}

#[test]
fn a_cell_keeps_its_rows_while_nothing_takes_them() {
    // Between rounds nothing has grown: letting the cell go would end the
    // region a row short of where it was painted.
    assert_eq!(plan_at_the_bottom(12, 0, &[cell(6, 5)]), [6]);
}

#[test]
fn a_cell_goes_the_frame_something_takes_its_rows() {
    // The next cell landed and the region is a row taller than it was
    // painted: the row the old cell gives up is paid for.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 18,
        pending_rows: 0,
        height: 13,
        holds: true,
    };
    assert_eq!(settle_plan(&geometry, &[cell(6, 5)]), [5]);
}

#[test]
fn a_cell_gives_up_only_the_rows_that_were_taken() {
    // A thought on four rows; the reply's first row and its separator took
    // two. It keeps one row of thought under its line.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 16,
        pending_rows: 0,
        height: 16,
        holds: true,
    };
    assert_eq!(settle_plan(&geometry, &[cell(4, 1)]), [2]);
}

#[test]
fn rows_already_queued_for_scrollback_count_as_taken() {
    // A commit this frame pushes the region down like any growth does.
    assert_eq!(plan_at_the_bottom(12, 1, &[cell(6, 5)]), [5]);
}

#[test]
fn the_oldest_cell_gives_its_rows_up_first() {
    // Scrollback takes cells in order, so the oldest must be the first down
    // to its committed height.
    let cells = [cell(4, 1), cell(6, 5)];
    // One row was taken: the oldest gives it.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 12,
        pending_rows: 0,
        height: 19,
        holds: true,
    };
    assert_eq!(settle_plan(&geometry, &cells), [3, 6]);
    // Three rows: the oldest is down to its line, the newest untouched.
    let geometry = SettleGeometry {
        height: 21,
        ..geometry
    };
    assert_eq!(settle_plan(&geometry, &cells), [1, 6]);
    // Four: the newest gives the last.
    let geometry = SettleGeometry {
        height: 22,
        ..geometry
    };
    assert_eq!(settle_plan(&geometry, &cells), [1, 5]);
}

#[test]
fn a_region_floating_above_the_bottom_is_held_where_it_was_painted() {
    // Early in a session the box floats; the same rule holds it: the region
    // ends where it was painted, not at the screen's last row.
    let geometry = SettleGeometry {
        painted_bottom: 20,
        view_top: 8,
        pending_rows: 0,
        height: 12,
        holds: true,
    };
    assert_eq!(settle_plan(&geometry, &[cell(6, 5)]), [6]);
    let grown = SettleGeometry {
        height: 13,
        ..geometry
    };
    assert_eq!(settle_plan(&grown, &[cell(6, 5)]), [5]);
}

#[test]
fn nothing_is_held_when_the_conversation_cannot_hold() {
    // The turn ended, a prompt replaced the region: every cell goes.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 18,
        pending_rows: 0,
        height: 12,
        holds: false,
    };
    assert_eq!(settle_plan(&geometry, &[cell(4, 1), cell(6, 5)]), [1, 5]);
}

#[test]
fn a_plan_never_asks_a_cell_for_more_rows_than_it_has() {
    // The region shrank for a reason no cell can cover (a toast expired):
    // the cells keep what they have and the box moves by the rest.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 20,
        pending_rows: 0,
        height: 8,
        holds: true,
    };
    assert_eq!(settle_plan(&geometry, &[cell(6, 5)]), [6]);
}

// ----- one frame ---------------------------------------------------------

#[test]
fn a_frame_lowers_the_cells_and_reports_the_ones_to_commit() {
    let mut app = App::new();
    app.begin_stream();
    app.push_settling(cell(4, 1));
    app.push_settling(cell(6, 5));
    // Three rows were taken: the thought is down to its line and commits,
    // the command keeps all six.
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 12,
        pending_rows: 0,
        height: 21,
        holds: true,
    };
    assert_eq!(settle_frame(&mut app, &geometry), 1);
    let rows: Vec<usize> = app.settling().iter().map(Settling::rows).collect();
    assert_eq!(rows, [6], "the committed cell left the strip");
}

#[test]
fn a_frame_keeps_a_settled_cell_behind_one_still_holding_rows() {
    // Scrollback order: a cell with nothing to give up still waits for the
    // one ahead of it.
    let mut app = App::new();
    app.begin_stream();
    app.push_settling(cell(6, 5));
    app.push_settling(Settling::settled(vec![Line::from("second")]));
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 16,
        pending_rows: 0,
        height: 14,
        holds: true,
    };
    assert_eq!(settle_frame(&mut app, &geometry), 0);
    assert_eq!(app.settling().len(), 2);
    // Something took the row: both go, in order.
    let grown = SettleGeometry {
        height: 15,
        ..geometry
    };
    assert_eq!(settle_frame(&mut app, &grown), 2);
    assert!(app.settling().is_empty());
}

#[test]
fn a_frame_with_nothing_settling_commits_nothing() {
    let mut app = App::new();
    let geometry = SettleGeometry {
        painted_bottom: 30,
        view_top: 25,
        pending_rows: 0,
        height: 5,
        holds: true,
    };
    assert_eq!(settle_frame(&mut app, &geometry), 0);
}

// ----- when a conversation may hold cells --------------------------------

#[test]
fn only_a_turn_in_flight_holds_cells() {
    let mut app = App::new();
    assert!(!settling_holds(&app, WIDTH, 30), "idle");
    app.begin_stream();
    assert!(settling_holds(&app, WIDTH, 30), "mid-turn");
    app.finish_stream();
    app.take_turn_summary(1);
    assert!(!settling_holds(&app, WIDTH, 30), "the turn ended");
}

#[test]
fn a_prompt_holds_nothing() {
    // While a permission prompt asks, the region is the prompt's and the
    // strip is not drawn: a cell kept there would be rows nothing paints,
    // measured out of a region that does not contain them
    // (docs/permissions.md). It commits, and scrolls in above the prompt.
    let mut app = App::new();
    app.begin_stream();
    assert!(settling_holds(&app, WIDTH, 30), "mid-turn");
    app.open_permission(crate::permission::PermissionRequest {
        id: "perm_0".to_string(),
        kind: crate::permission::PermissionKind::Bash,
        target: "make test".to_string(),
        body: String::new(),
        detail: None,
        agent: None,
        agent_id: None,
    });
    assert!(!settling_holds(&app, WIDTH, 30), "a prompt is open");
    // …and a cell that ends behind it does not settle in place.
    assert!(!settles_in_place(&app, &cell(6, 5), WIDTH, 30));
}

#[test]
fn an_overlay_holds_nothing() {
    // Under Ctrl+O or Ctrl+D nothing inline is painted: commits queue for
    // the return, as they always have (invariant 4), and a cell kept in a
    // strip nobody sees would only delay its lines.
    let mut app = App::new();
    app.begin_stream();
    assert!(settling_holds(&app, WIDTH, 30), "mid-turn");
    app.view = crate::app::View::ToolOutput;
    assert!(!settling_holds(&app, WIDTH, 30), "the transcript is up");
    app.view = crate::app::View::Conversation;
    assert!(settling_holds(&app, WIDTH, 30), "back on the conversation");
}

#[test]
fn a_terminal_too_short_for_the_strip_holds_nothing() {
    // Where the strip is already squeezed its overflow flows into scrollback
    // (docs/strip-flow.md) and a held cell would be rows the region does
    // not have.
    let mut app = running(9);
    let (cell, _) = settle_command(&mut app, "Exit code: 0\nx\n", true, WIDTH);
    app.push_settling(cell);
    assert!(settling_holds(&app, WIDTH, 30));
    assert!(!settling_holds(&app, WIDTH, 9), "nine rows cannot hold it");
}

#[test]
fn a_cell_settles_in_place_only_when_it_has_rows_to_give_or_one_ahead_of_it() {
    let mut app = App::new();
    app.begin_stream();
    let short = cell(6, 5);
    let even = Settling::settled(vec![Line::from("as tall as it ran")]);
    assert!(settles_in_place(&app, &short, WIDTH, 30));
    assert!(
        !settles_in_place(&app, &even, WIDTH, 30),
        "nothing to hold: it commits at once"
    );
    app.push_settling(short);
    assert!(
        settles_in_place(&app, &even, WIDTH, 30),
        "it waits its turn behind the cell ahead of it"
    );
    // Idle, nothing is held.
    let idle = App::new();
    assert!(!settles_in_place(&idle, &cell(6, 5), WIDTH, 30));
}
