//! A live cell is never taller than what it leaves behind
//! (`docs/live-cells.md`).
//!
//! The live region is content-anchored: rows committed to scrollback push
//! the box down, rows the strip gives up pull it back. So a cell that stood
//! taller live than the rows it commits lifted the box the moment it
//! settled, and the next cell dropped it again — the jump between a thought
//! and its tool call, between one command and the next. These tests hold
//! every live cell to the height it settles into, and then play whole
//! hand-offs the way the boundary does.

use super::*;
use crate::app::{ToolPlacement, group_kind};
use crate::tasks::TaskStore;
use crate::ui::live::preview_tool_lines;
use crate::ui::theme::{TOOL_BACKGROUND_HINT, tool_dim_color, tool_output_color};
use crate::ui::tool::running_command_lines;
use crate::ui::tool_group::live_group_lines;

/// `n` one-row lines, `line 1` … `line n`.
fn numbered(n: usize) -> String {
    (1..=n)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The running cell of a `Bash` call that has printed `output` so far.
fn running_cell(output: &str, width: u16, hint: Option<&'static str>) -> Vec<Line<'static>> {
    let call = tool("Bash", "cmd", ToolStatus::Running, output);
    running_command_lines(
        &call,
        &crate::app::LiveTail::default(),
        Duration::from_secs(9),
        Duration::ZERO,
        width,
        &PathDisplay::VERBATIM,
        hint,
    )
}

fn running_rows(output: &str, width: u16, hint: Option<&'static str>) -> Vec<String> {
    running_cell(output, width, hint)
        .iter()
        .map(plain)
        .collect()
}

/// The cell the same call commits if it ends on that output.
fn settled_rows(output: &str, width: u16) -> Vec<String> {
    tool_lines(
        &tool("Bash", "cmd", ToolStatus::Ok, output),
        width,
        &PathDisplay::VERBATIM,
    )
    .iter()
    .map(plain)
    .collect()
}

/// Outputs of every shape the settled fold treats differently.
fn output_shapes() -> Vec<String> {
    let mut outputs: Vec<String> = (0..=12).map(numbered).collect();
    outputs.extend([
        // A first block of one row, then a blank line: the fold stops there.
        "title\n\nrow 1\nrow 2\nrow 3\nrow 4".to_string(),
        // …of two rows.
        "one\ntwo\n\nrow 1\nrow 2\nrow 3\nrow 4".to_string(),
        // Leading blank lines, which the fold skips.
        "\n\nlate start\nmore\nand more\nand more\nlast".to_string(),
        // A blank line inside an output short enough to show whole.
        "a\n\nb".to_string(),
        // Trailing blank lines, which the settled cell drops.
        "only\n\n\n".to_string(),
        // Nothing but blank lines: the settled cell reads `(no output)`.
        "\n\n\n".to_string(),
        // A line that wraps past the fold by itself.
        format!("{}\nshort", "x".repeat(300)),
        // One line, several rows, nothing to fold.
        "word ".repeat(30),
        // A row as wide as the cell: no room beside it for a clock.
        "y".repeat(75),
        // A JSON line the settled cell reshapes into several rows…
        "{\"a\":1,\"b\":[1,2,3]}".to_string(),
        // …and one it reshapes into *fewer* rows than it printed: an empty
        // document padded past the width settles as `[]`, one row.
        format!("[{}]", " ".repeat(200)),
        format!("{{}}{}\nnext", " ".repeat(200)),
        // A JSON line opening a first block the fold closes at once…
        "{\"a\":1}\n\nrow 1\nrow 2\nrow 3".to_string(),
        // …and a line that only *looks* like one once its tab is expanded
        // for display: the settled cell reshapes the line as it was printed,
        // where a raw tab inside a string is no JSON, so it stays one row.
        "{\"a\":\"x\ty\"}\n\nrow 1\nrow 2\nrow 3".to_string(),
    ]);
    outputs
}

#[test]
fn a_running_command_never_stands_taller_than_it_would_settle() {
    // The reported jump: a running command tailed four rows over its clock
    // row and settled into three over the hint — or into one, when a blank
    // line closed the peek early — so the box rose as each command ended and
    // dropped back when the next cell arrived. Whatever the output's shape,
    // the running cell is no taller than the cell it would commit right now.
    for output in output_shapes() {
        for width in [24, 40, 80, 120] {
            for hint in [None, Some(TOOL_BACKGROUND_HINT)] {
                let live = running_rows(&output, width, hint);
                let settled = settled_rows(&output, width);
                assert!(
                    live.len() <= settled.len(),
                    "width {width}, hint {hint:?}, output {output:?}\n live: {live:#?}\n settled: {settled:#?}"
                );
            }
        }
    }
}

#[test]
fn a_running_command_stands_exactly_as_tall_as_the_cell_it_settles_into() {
    // …and no shorter than it has to be: a single run of lines, short or
    // long, and anything already past the fold, stands on exactly the rows
    // it settles into — the tail is the peek's height, not a row under it.
    let mut outputs: Vec<String> = (0..=12).map(numbered).collect();
    outputs.extend([
        "title\n\nrow 1\nrow 2\nrow 3\nrow 4".to_string(),
        "one\ntwo\n\nrow 1\nrow 2\nrow 3\nrow 4".to_string(),
        "\n\nlate start\nmore\nand more\nand more\nlast".to_string(),
        "only\n\n\n".to_string(),
        "\n\n\n".to_string(),
        format!("{}\nshort", "x".repeat(300)),
        "word ".repeat(30),
        "y".repeat(75),
    ]);
    for output in outputs {
        for width in [24, 40, 80, 120] {
            for hint in [None, Some(TOOL_BACKGROUND_HINT)] {
                let live = running_rows(&output, width, hint);
                let settled = settled_rows(&output, width);
                assert_eq!(
                    live.len(),
                    settled.len(),
                    "width {width}, hint {hint:?}, output {output:?}\n live: {live:#?}\n settled: {settled:#?}"
                );
            }
        }
    }
}

#[test]
fn a_running_cell_only_grows_and_never_past_the_cell_it_becomes() {
    // Two ways a tail could still move the box: by shrinking while the
    // command runs, and by standing taller than the cell the finished
    // output settles into. The settled fold can be *shorter* than the short
    // output it replaces — `title`, a blank line and a row show as three
    // rows, and as `title` over the hint once the output runs past the fold
    // — so the tail stands on the rows it is sure of. Every prefix of each
    // output, in order: the height never drops and never passes the end's.
    let outputs = [
        "title\n\nrow 1\nrow 2\nrow 3\nrow 4\n\nfooter line\nlast".to_string(),
        "\n\nlate start\nmore\n\nand more\nand more\nlast".to_string(),
        "one\ntwo\n\nthree".to_string(),
        "a\n\nb".to_string(),
        numbered(9),
        format!("{}\n\n{}\nend", "x".repeat(200), "word ".repeat(40)),
        "{\"a\":1,\"b\":[1,2,3]}\nplain\n\nmore\nand more\nand more".to_string(),
        "plain\n\n{\"a\":1,\"b\":[1,2,3]}\nmore".to_string(),
        "\n{\"a\":\"x\ty\"}\n\nrow 1\nrow 2\nrow 3".to_string(),
    ];
    for full in &outputs {
        for width in [30, 80] {
            let settled = settled_rows(full, width).len();
            let mut before = 0;
            for end in 0..=full.len() {
                if !full.is_char_boundary(end) {
                    continue;
                }
                let live = running_rows(&full[..end], width, Some(TOOL_BACKGROUND_HINT)).len();
                assert!(
                    live >= before,
                    "width {width}: the running cell shrank from {before} rows to {live} at {:?}",
                    &full[..end]
                );
                assert!(
                    live <= settled,
                    "width {width}: after {:?} the running cell is {live} rows, taller than \
                     the {settled} it settles into",
                    &full[..end]
                );
                before = live;
            }
        }
    }
}

#[test]
fn a_long_output_tails_three_rows_over_the_clock_row() {
    // The settled fold's own budget — three rows over one closing row — so
    // the tail and the peek it becomes are one height.
    let rows = running_rows(&numbered(9), 80, None);
    assert_eq!(
        rows[1..],
        [
            "  ⎿  line 7",
            "     line 8",
            "     line 9",
            "     +6 lines (9s · wait 2m)",
        ],
        "{rows:#?}"
    );
}

#[test]
fn an_output_that_fits_shows_whole_with_the_clock_on_its_last_row() {
    // The settled cell shows these rows and nothing under them, so the clock
    // has no row of its own to stand on: it rides the end of the last row —
    // the read group's own rule for its command row (docs/tool-groups.md).
    let rows = running_rows("alpha\nbeta", 80, None);
    assert_eq!(
        rows[1..],
        ["  ⎿  alpha", "     beta (9s · wait 2m)"],
        "{rows:#?}"
    );
    // Four rows are still whole: the settled cell hides nothing of them.
    let four = running_rows(&numbered(4), 80, None);
    assert_eq!(
        four[1..],
        [
            "  ⎿  line 1",
            "     line 2",
            "     line 3",
            "     line 4 (9s · wait 2m)",
        ],
        "{four:#?}"
    );
}

#[test]
fn the_clock_on_an_output_row_keeps_its_own_ink() {
    // Output is the reply's ink and the clock is meta: the row carries both,
    // each in its own.
    let cell = running_cell("alpha", 80, None);
    let spans = &cell[1].spans;
    let text = spans
        .iter()
        .find(|span| span.content == "alpha")
        .expect("the output's own span");
    assert_eq!(text.style.fg, Some(tool_output_color()));
    let clock = spans.last().expect("the row has spans");
    assert_eq!(clock.content, " (9s · wait 2m)");
    assert_eq!(clock.style.fg, Some(tool_dim_color()));
}

#[test]
fn a_last_row_with_no_room_for_the_clock_shows_whole_without_it() {
    // Output is never cut to seat the clock: what a command just printed
    // ends at the right edge — a progress bar's rate and its ETA — and the
    // status line still counts the turn. The row stays one row either way.
    let bar = format!("{} 12.3/45.6 MB 5.2 MB/s eta 0:00:07", "━".repeat(40));
    let rows = running_rows(&bar, 80, None);
    assert_eq!(rows[1..], [format!("  ⎿  {bar}")], "{rows:#?}");
    // One column short of the clock and its space is still no room…
    let tight = "y".repeat(75 - 14);
    assert_eq!(
        running_rows(&tight, 80, None)[1..],
        [format!("  ⎿  {tight}")]
    );
    // …and exactly enough is room.
    let fits = "y".repeat(75 - 15);
    assert_eq!(
        running_rows(&fits, 80, None)[1..],
        [format!("  ⎿  {fits} (9s · wait 2m)")]
    );
    // A terminal too narrow for any clock keeps the output the same way.
    let narrow = running_rows("line 1", 24, None);
    assert_eq!(narrow[1..], ["  ⎿  line 1"], "{narrow:#?}");
}

#[test]
fn a_blank_line_early_in_the_output_narrows_the_window_with_the_fold() {
    // The settled peek is the output's first block, so this one settles into
    // `title` over the hint — two rows — and the tail keeps to two: the
    // newest row over the clock row.
    let rows = running_rows("title\n\nrow 1\nrow 2\nrow 3\nrow 4", 80, None);
    assert_eq!(
        rows[1..],
        ["  ⎿  row 4", "     +5 lines (9s · wait 2m)"],
        "{rows:#?}"
    );
    let settled = settled_rows("title\n\nrow 1\nrow 2\nrow 3\nrow 4", 80);
    assert_eq!(
        settled[1..],
        ["  ⎿  title", "     … +5 lines (ctrl+o to expand)"],
        "{settled:#?}"
    );
}

#[test]
fn a_cell_sure_of_one_row_only_counts_what_it_cannot_show() {
    // The settled cell reshapes a JSON line, and an empty document padded
    // past the width comes out *shorter* than it printed: two rows of
    // brackets and spaces settle as `[]`. One row is all the running cell
    // can count on, so the clock row stands alone and counts every row.
    let output = format!("[{}]", " ".repeat(200));
    let rows = running_rows(&output, 80, None);
    assert_eq!(rows[1..], ["  ⎿  +2 lines (9s · wait 2m)"], "{rows:#?}");
    let settled = settled_rows(&output, 80);
    assert_eq!(settled[1..], ["  ⎿  []"], "{settled:#?}");
}

#[test]
fn trailing_blank_lines_are_nothing_to_tail() {
    // A command's closing newlines used to take rows of the window; the
    // settled cell drops them, and so does the tail.
    let rows = running_rows("only\n\n\n", 80, None);
    assert_eq!(rows[1..], ["  ⎿  only (9s · wait 2m)"], "{rows:#?}");
    // Blank lines and nothing else: still a silent command.
    let silent = running_rows("\n\n\n", 80, None);
    assert_eq!(silent[1..], ["  ⎿  Running… (9s · wait 2m)"], "{silent:#?}");
}

#[test]
fn the_ctrl_b_hint_rides_the_clock_row_instead_of_adding_one() {
    // The hint used to be a row of its own under the cell — a row the
    // settled cell never has, so every command that ran long enough to show
    // it lifted the box a row when it ended.
    let hint = Some(TOOL_BACKGROUND_HINT);
    let folded = running_rows(&numbered(9), 80, hint);
    assert_eq!(
        folded.last().map(String::as_str),
        Some("     +6 lines (9s · wait 2m) (ctrl+b to run in background)")
    );
    let whole = running_rows("alpha\nbeta", 80, hint);
    assert_eq!(
        whole.last().map(String::as_str),
        Some("     beta (9s · wait 2m) (ctrl+b to run in background)")
    );
    let silent = running_rows("", 80, hint);
    assert_eq!(
        silent[1..],
        ["  ⎿  Running… (9s · wait 2m) (ctrl+b to run in background)"]
    );
}

#[test]
fn a_hint_the_clock_row_has_no_room_for_is_left_out() {
    // The key works either way; a hint that wrapped would cost the row it
    // was folded in to save, and one cut mid-word says nothing.
    let hint = Some(TOOL_BACKGROUND_HINT);
    for width in [24, 40, 50] {
        for output in [String::new(), "alpha\nbeta".to_string(), numbered(9)] {
            let rows = running_rows(&output, width, hint);
            assert_eq!(
                rows,
                running_rows(&output, width, None),
                "width {width}: the hint does not fit and is left out whole"
            );
        }
    }
}

// --- the live-only rows of the other cells ---

fn texts(lines: &[Line<'static>]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

/// One call of a batch as the backend announces it.
fn call(name: &str, args: &str) -> crate::stream::ToolCallSummary {
    crate::stream::ToolCallSummary {
        name: name.to_string(),
        args: args.to_string(),
    }
}

#[test]
fn the_groups_ctrl_b_hint_rides_its_command_row() {
    // A long search in the collapsed group used to grow a hint row under
    // the cell after three seconds and lose it the moment the search ended,
    // the group still open: a row up, a row down, the box with it.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "grep -rn todo .", None);
    app.set_command_elapsed(Some(Duration::from_secs(12)));
    let running = texts(&preview_tool_lines(&app, 80));
    assert_eq!(
        running[1..],
        ["  ⎿  $ grep -rn todo . (12s) (ctrl+b to run in background)"],
        "{running:#?}"
    );
    // The search ends and the group stays open on the same rows.
    app.end_tool("a.rs:1: todo", true).expect("running");
    assert_eq!(app.place_resolved_tool(), crate::app::ToolPlacement::Held);
    let held = texts(&live_group_lines(&app, 80, None, true, true));
    assert_eq!(held.len(), running.len(), "{held:#?}");
}

#[test]
fn a_group_clock_with_no_room_on_the_row_is_left_out() {
    // It used to take a row of its own under the command — a row the cell
    // gave back when the command ended.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool("Bash", "grep -rn todo .", None);
    app.push_tool_output("a.rs:1: todo\nb.rs:2: todo\n");
    app.set_command_elapsed(Some(Duration::from_secs(65)));
    let wide = texts(&live_group_lines(&app, 60, None, true, true));
    assert_eq!(wide[1..], ["  ⎿  $ grep -rn todo . (1m 5s · 2 lines)"]);
    let narrow = texts(&live_group_lines(&app, 34, None, true, true));
    assert_eq!(narrow[1..], ["  ⎿  $ grep -rn todo ."], "{narrow:#?}");
}

#[test]
fn the_group_keeps_its_hint_row_only_while_it_has_the_strip_to_itself() {
    // A read leading a batch whose other calls are no reads: the group
    // closes the moment the next call starts, and its one settled line is a
    // row shorter than a header over a hint — with the batch's other cells
    // still up, nothing took that row and the box rose by it. Beside other
    // cells the group is its header alone, the height of the line it becomes.
    let mut app = App::new();
    app.begin_stream();
    app.start_tool_batch(&[call("Read", "a.py"), call("Bash", "make build")]);
    app.start_tool("Read", "a.py", None);
    let rows = texts(&preview_tool_lines(&app, 80));
    assert_eq!(
        rows,
        [
            "● Reading 1 file… (ctrl+o to expand)",
            "",
            "● Bash(make build)",
            "  ⎿  Waiting…",
        ],
        "{rows:#?}"
    );
    assert_eq!(usize::from(preview_rows(&app, 80)), rows.len());
    // Alone, the row is back: a batch of reads and nothing else.
    let mut alone = App::new();
    alone.begin_stream();
    alone.start_tool_batch(&[call("Read", "a.py")]);
    alone.start_tool("Read", "a.py", None);
    assert_eq!(
        texts(&preview_tool_lines(&alone, 80)),
        ["● Reading 1 file… (ctrl+o to expand)", "  ⎿  a.py"]
    );
}

#[test]
fn the_agent_trees_ctrl_b_hint_rides_its_header() {
    // The tree settles into a header over the same two rows an agent — the
    // hint row under it had no row to become.
    let mut app = App::new();
    app.begin_stream();
    app.start_agent_group(false, &[spec("a1", "One", false), spec("a2", "Two", false)]);
    let early = texts(&live_agent_group_lines(&app, 80));
    app.set_command_elapsed(Some(Duration::from_secs(3)));
    let hinted = texts(&live_agent_group_lines(&app, 80));
    assert_eq!(hinted.len(), early.len(), "{hinted:#?}");
    assert_eq!(
        hinted[0],
        "● Running 2 agents… (ctrl+o to expand) (ctrl+b to run in background)"
    );
    // Too narrow for both hints: the Ctrl+B one sits out.
    assert_eq!(
        texts(&live_agent_group_lines(&app, 50))[0],
        "● Running 2 agents… (ctrl+o to expand)"
    );
}

#[test]
fn a_lone_agents_ctrl_b_hint_rides_its_header_too() {
    // Moved to the background, a lone agent settles into two rows — its
    // header over `Running in the background` — so a third live row was a
    // row given up at the keypress the hint asked for.
    let mut app = App::new();
    app.begin_stream();
    app.start_agent_group(false, &[spec("a1", "Find the bug", false)]);
    app.set_command_elapsed(Some(Duration::from_secs(3)));
    let rows = texts(&live_agent_group_lines(&app, 80));
    assert_eq!(rows.len(), 2, "{rows:#?}");
    assert_eq!(
        rows[0],
        "● Agent(Find the bug) (ctrl+b to run in background)"
    );
}

// --- whole hand-offs, played the way the event loop plays them ---

/// A turn folded event by event the way `tui::stream` folds it — each step
/// applies one event to the `App` and counts the rows it queues for
/// scrollback — with the one thing this file is about checked at every
/// step: **the box may not rise**.
///
/// The live region is content-anchored, so between two frames the box moves
/// down by the rows committed and up by the rows the region gave back. A
/// step whose commits do not cover the rows the strip lost is a frame in
/// which the box sits higher than it did, until something pushes it back
/// down — the jump this file exists to keep out.
struct Stage {
    app: App,
    render: StreamRender,
    width: u16,
}

impl Stage {
    fn new(width: u16) -> Self {
        let mut app = App::new();
        app.begin_stream();
        app.set_status_times(Duration::from_secs(5), None);
        Self {
            app,
            render: StreamRender::new(),
            width,
        }
    }

    /// The live region's rows as the next frame draws them, on a terminal
    /// tall enough to hold it whole.
    fn region(&mut self) -> usize {
        // The boundary injects the reply frontier's height before every draw
        // (`Session::stream_preview_lines`).
        let frontier = match self.app.streaming_text() {
            Some(text) if !text.is_empty() && self.app.current_tool().is_none() => {
                let text = text.to_string();
                self.render.preview(&text, self.width, 200).len()
            }
            _ => 1,
        };
        self.app
            .set_stream_preview_rows(u16::try_from(frontier).unwrap_or(u16::MAX));
        let (app, width) = (&self.app, self.width);
        let band = band_rows(app, width);
        usize::from(live_height(
            &app.input,
            width,
            500,
            strip_has_status(app),
            preview_rows(app, width),
            hang_rows(app, width),
            queued_rows(app, width),
            toast_rows(app),
            band,
            footer_rows(app, band),
            agent_list_rows(app),
        ))
    }

    /// Fold one event: `fold` applies it and returns the rows it commits.
    fn step(&mut self, what: &str, fold: impl FnOnce(&mut Self) -> usize) {
        let before = self.region();
        let committed = fold(self);
        let after = self.region();
        assert!(
            committed + after >= before,
            "{what}: the box rose {} row(s) — a region of {before} rows became {after} \
             with {committed} committed",
            before - committed - after
        );
    }

    fn close_group(&mut self) -> usize {
        let (width, paths) = (self.width, self.app.path_display().clone());
        self.app.close_tool_group().map_or(0, |range| {
            tool_group_lines(&self.app.history[range], width, &paths).len() + 1
        })
    }

    fn close_group_unless_the_front_reads(&mut self) -> usize {
        let joins = self
            .app
            .tool_queue()
            .front()
            .is_some_and(|call| group_kind(call, self.app.path_display()).is_some());
        if joins { 0 } else { self.close_group() }
    }

    /// `Session::flush_segment`: the run of reply text before an interleaved
    /// cell becomes its own message, its last line and a spacer committed.
    fn flush_segment(&mut self) -> usize {
        let mut rows = 0;
        if self
            .app
            .streaming_text()
            .is_some_and(|text| !text.trim().is_empty())
        {
            rows += self.close_group();
        }
        if let Some(segment) = self.app.flush_streaming_segment() {
            rows += self.render.finish(&segment, self.width).len() + 1;
        }
        self.render.reset();
        rows
    }

    /// `ThinkingStart`.
    fn thinking_start(&mut self) -> usize {
        let rows = self.flush_segment();
        self.app.begin_reasoning();
        rows
    }

    /// `ThinkingEnd` — `Session::settle_reasoning`.
    fn thinking_end(&mut self) -> usize {
        if self
            .app
            .reasoning()
            .is_none_or(|text| text.trim().is_empty())
        {
            self.app.finish_reasoning(1);
            return 0;
        }
        let rows = self.flush_segment();
        let Some(thought) = self.app.finish_reasoning(1) else {
            return rows;
        };
        if self.app.tool_group_range().is_some() {
            return rows;
        }
        rows + reasoning_lines(&thought, self.width).len() + 1
    }

    /// `Chunk`.
    fn chunk(&mut self, text: &str) -> usize {
        self.app.push_chunk(text);
        let mut rows = 0;
        if self
            .app
            .streaming_text()
            .is_some_and(|text| !text.trim().is_empty())
        {
            rows += self.close_group();
        }
        if let Some(text) = self.app.streaming_text() {
            let text = text.to_string();
            rows += self.render.commit(&text, self.width).len();
        }
        rows
    }

    /// `ToolBatch`.
    fn tool_batch(&mut self, calls: &[(&str, &str)]) -> usize {
        let rows = self.flush_segment();
        let items: Vec<_> = calls.iter().map(|(name, args)| call(name, args)).collect();
        self.app.start_tool_batch(&items);
        rows + self.close_group_unless_the_front_reads()
    }

    /// `ToolStart`.
    fn tool_start(&mut self, name: &str, args: &str) -> usize {
        let rows = self.flush_segment();
        self.app.start_tool(name, args, None);
        self.app.set_command_elapsed(Some(Duration::ZERO));
        rows + self.close_group_unless_the_front_reads()
    }

    /// `ToolEnd` — `Session::commit_tool_cell`.
    fn tool_end(&mut self, output: &str, ok: bool) -> usize {
        if self.app.end_tool(output, ok).is_none() {
            return 0;
        }
        self.app.set_command_elapsed(None);
        let mut rows = 0;
        match self.app.place_resolved_tool() {
            ToolPlacement::Held => return 0,
            ToolPlacement::Commit { closed } => {
                if let Some(range) = closed {
                    let paths = self.app.path_display().clone();
                    rows +=
                        tool_group_lines(&self.app.history[range], self.width, &paths).len() + 1;
                }
            }
        }
        if let Some(lines) = tool_commit_lines(
            &self.app.history,
            self.app.tool_queue(),
            self.width,
            self.app.path_display(),
        ) && !lines.is_empty()
        {
            rows += lines.len() + 1;
        }
        rows
    }

    /// `TaskCall`: a task tool has no cell, only the checklist.
    fn task_call(&mut self, tasks: TaskStore) -> usize {
        let rows = self.flush_segment();
        self.app
            .record_task_call("TaskCreate", "a", "{}", "Task #1 created", true, tasks);
        rows
    }

    /// A whole command, start to settle: its output arriving a line at a
    /// time, the Ctrl+B hint falling due halfway through.
    fn run_command(&mut self, command: &str, output: &str) {
        self.step("the command starts", |s| s.tool_start("Bash", command));
        let lines: Vec<&str> = output.split_inclusive('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if i == lines.len() / 2 {
                self.step("the Ctrl+B hint falls due", |s| {
                    s.app.set_command_elapsed(Some(Duration::from_secs(5)));
                    0
                });
            }
            self.step("the command prints", |s| {
                s.app.push_tool_output(line);
                0
            });
        }
        self.step("the Ctrl+B hint is due", |s| {
            s.app.set_command_elapsed(Some(Duration::from_secs(9)));
            0
        });
        let settled = format!("Exit code: 0\n{output}");
        self.step("the command ends", |s| s.tool_end(&settled, true));
    }
}

/// A thought long enough to fill any block the strip ever gave it.
const LONG_THOUGHT: &str = "The user wants the answer, and the way to get it is to look first.\n\
    There are a few places it could be, so the plan is to check each of them in turn.\n\
    Once that is done the result can be read back and reported in a sentence or two.\n\
    Nothing else is needed after that, so the turn can end there.";

fn one_task() -> TaskStore {
    let mut store = TaskStore::new();
    store
        .run_create(r#"{"subject":"Count the lines","description":"d"}"#)
        .expect("create succeeds");
    store
}

#[test]
fn a_thought_gives_way_to_its_tool_call_without_the_box_rising() {
    // The report: thinking, a jump, the tool's cell. The phase ends at the
    // call's first piece and the call has no cell until its arguments have
    // streamed — a round trip of rows for as long as that took.
    for width in [40, 80, 120] {
        let mut stage = Stage::new(width);
        stage.step("the phase opens", Stage::thinking_start);
        stage.step("the model thinks", |s| {
            s.app.push_thinking(LONG_THOUGHT);
            0
        });
        stage.step(
            "the phase ends at the call's first piece",
            Stage::thinking_end,
        );
        stage.step("the call is announced", |s| {
            s.tool_batch(&[("Skill", "web-search")])
        });
        stage.step("the call starts", |s| s.tool_start("Skill", "web-search"));
        stage.step("the call ends", |s| {
            s.tool_end("Successfully loaded skill", true)
        });
        // …and thinking again, the next round.
        stage.step("the next phase opens", Stage::thinking_start);
        stage.step("the model thinks", |s| {
            s.app.push_thinking(LONG_THOUGHT);
            0
        });
        stage.step("the next phase ends", Stage::thinking_end);
    }
}

#[test]
fn a_thought_gives_way_to_the_reply_without_the_box_rising() {
    for width in [40, 80, 120] {
        let mut stage = Stage::new(width);
        stage.step("the phase opens", Stage::thinking_start);
        stage.step("the model thinks", |s| {
            s.app.push_thinking(LONG_THOUGHT);
            0
        });
        stage.step(
            "the phase ends at the reply's first token",
            Stage::thinking_end,
        );
        for piece in [
            "Here",
            " is the answer, long enough to wrap onto a second row of any",
            " terminal this narrow.\n\nAnd a second paragraph",
            " after it.",
        ] {
            stage.step("the reply streams", |s| s.chunk(piece));
        }
    }
}

#[test]
fn a_thought_followed_by_a_call_with_no_cell_leaves_the_box_where_it_is() {
    // A task call has no cell: nothing lands on the rows a thought stood
    // on, so a block taller than its settled line gave them up for good —
    // every `taskupdate` after a think.
    let mut stage = Stage::new(80);
    stage.step("the phase opens", Stage::thinking_start);
    stage.step("the model thinks", |s| {
        s.app.push_thinking(LONG_THOUGHT);
        0
    });
    stage.step("the phase ends", Stage::thinking_end);
    stage.step("the task call lands", |s| s.task_call(one_task()));
}

#[test]
fn a_reply_gives_way_to_a_thought_without_the_box_rising() {
    // A model that reasons after it has begun to answer: the block goes up
    // over the paragraph that was streaming.
    let mut stage = Stage::new(80);
    stage.step("the reply streams", |s| s.chunk("Let me look at that"));
    stage.step("the reply streams", |s| s.chunk(" before I answer."));
    stage.step("a phase opens", Stage::thinking_start);
    stage.step("the model thinks", |s| {
        s.app.push_thinking(LONG_THOUGHT);
        0
    });
    stage.step("the phase ends", Stage::thinking_end);
}

#[test]
fn a_command_runs_and_settles_without_the_box_rising() {
    // Bash, a jump, bash: every shape of output, printed a line at a time
    // with the Ctrl+B hint falling due on the way, then settled.
    let numbered_lines = |n: usize| (1..=n).map(|i| format!("line {i}\n")).collect::<String>();
    let outputs = [
        numbered_lines(30),
        numbered_lines(4),
        numbered_lines(2),
        "only\n".to_string(),
        String::new(),
        format!("title\n\n{}", numbered_lines(12)),
        format!("\n> app@1.0.0 build\n> vite build\n\n{}", numbered_lines(9)),
        format!("{}\n", "wide ".repeat(60)),
        "a\n\nb\n".to_string(),
    ];
    for output in &outputs {
        for width in [40, 80, 120] {
            let mut stage = Stage::new(width);
            stage.step("the call is announced", |s| {
                s.tool_batch(&[("Bash", "make build")])
            });
            stage.run_command("make build", output);
            // The round boundary: nothing lands until the next request
            // answers, and the box has to be where the settle left it.
            stage.step("the next round thinks", Stage::thinking_start);
        }
    }
}

#[test]
fn a_parallel_batch_settles_call_by_call_without_the_box_rising() {
    let mut stage = Stage::new(80);
    stage.step("the batch is announced", |s| {
        s.tool_batch(&[
            ("Bash", "make build"),
            ("Bash", "make test"),
            ("Bash", "make lint"),
        ])
    });
    stage.run_command("make build", &"compiling\n".repeat(20));
    stage.run_command("make test", "title\n\nrow 1\nrow 2\nrow 3\nrow 4\n");
    stage.run_command("make lint", "ok\n");
}

#[test]
fn a_read_group_closes_under_the_rest_of_its_batch_without_the_box_rising() {
    // A read leading a batch of other calls: the group closes when the next
    // call starts, the batch's other cells still up — nothing lands on the
    // row a hint under the group's header would have stood on.
    let mut stage = Stage::new(80);
    stage.step("the batch is announced", |s| {
        s.tool_batch(&[
            ("Read", "about.py"),
            ("Edit", "about.py"),
            ("Bash", "make build"),
        ])
    });
    stage.step("the read starts", |s| s.tool_start("Read", "about.py"));
    stage.step("the read ends and is held", |s| s.tool_end("1 x", true));
    stage.step("the edit starts and the group closes", |s| {
        s.tool_start("Edit", "about.py")
    });
    stage.step("the edit ends", |s| {
        s.tool_end("Updated about.py (+1 -1)\n 1 -a\n 1 +b", true)
    });
    stage.run_command("make build", "ok\n");
}

#[test]
fn an_open_read_group_gives_way_to_whatever_ends_it_without_the_box_rising() {
    // Between rounds the group has the strip to itself, hint row and all:
    // what ends it lands on those rows.
    let held = |width: u16| {
        let mut stage = Stage::new(width);
        stage.step("the batch is announced", |s| {
            s.tool_batch(&[("Read", "about.py")])
        });
        stage.step("the read starts", |s| s.tool_start("Read", "about.py"));
        stage.step("the read ends and is held", |s| s.tool_end("1 x", true));
        stage
    };
    for width in [40, 80] {
        // …a reply,
        let mut stage = held(width);
        stage.step("the reply's first word ends the group", |s| {
            s.chunk("Found")
        });
        stage.step("the reply streams", |s| s.chunk(" it in about.py."));
        // …a call that is no read,
        let mut stage = held(width);
        stage.step("a command's batch ends the group", |s| {
            s.tool_batch(&[("Bash", "make build")])
        });
        stage.run_command("make build", "ok\n");
        // …a thought, which opens no block over the group and settles into it,
        let mut stage = held(width);
        stage.step("a phase opens over the group", Stage::thinking_start);
        stage.step("the model thinks", |s| {
            s.app.push_thinking(LONG_THOUGHT);
            0
        });
        stage.step("the phase ends, held with the group", Stage::thinking_end);
        // …and another read, which joins it.
        stage.step("another read's batch joins the group", |s| {
            s.tool_batch(&[("Read", "b.py")])
        });
        stage.step("the read starts", |s| s.tool_start("Read", "b.py"));
        stage.step("the read ends and is held", |s| s.tool_end("1 y", true));
    }
}

#[test]
fn a_write_streaming_under_an_open_group_never_takes_rows_it_gives_back() {
    // The write's first pieces carry no path yet, so nothing says whether
    // it is the group's own. A cell put up meanwhile was two rows the strip
    // gave back when the path turned out to be the scratchpad's.
    let piece = |arguments: &str| crate::stream::ToolCallFragment {
        index: 0,
        name: "write".to_string(),
        arguments: arguments.to_string(),
    };
    for path in [
        "/tmp/alter-zero-0/s1/scratchpad/notes.md",
        "/tmp/elsewhere/notes.md",
    ] {
        let mut stage = Stage::new(80);
        stage.step("the batch is announced", |s| {
            s.tool_batch(&[("Read", "about.py")])
        });
        stage.step("the read starts", |s| s.tool_start("Read", "about.py"));
        stage.step("the read ends and is held", |s| s.tool_end("1 x", true));
        stage.step("the write's first piece", |s| {
            s.app.push_tool_call_fragment(&piece(r#"{"pa"#));
            0
        });
        stage.step("its path arrives", |s| {
            s.app
                .push_tool_call_fragment(&piece(&format!(r#"th":"{path}","content":"a\nb"#)));
            if s.app.draft_ends_group() {
                s.close_group()
            } else {
                0
            }
        });
        stage.step("its content streams", |s| {
            s.app.push_tool_call_fragment(&piece(r"\nc\nd"));
            0
        });
    }
}

#[test]
fn an_agents_session_view_holds_its_cells_to_the_same_heights() {
    // The viewed agent's strip is drawn by the same renderers: its thought
    // is one row, and its running command is no taller than it settles.
    use crate::stream::StreamEvent;
    let mut app = App::new();
    app.begin_stream();
    app.start_agent_group(false, &[spec("a1", "Build it", false)]);
    app.open_agent_view("a1");
    // The boundary opens the phase (the `/settings` Hide thinking gate) and
    // settles it into the agent's own transcript.
    app.begin_agent_reasoning("a1");
    app.apply_agent_event("a1", &StreamEvent::ThinkingChunk(LONG_THOUGHT.to_string()));
    assert_eq!(preview_rows(&app, 80), 1, "the agent's thought is one row");
    let thought = app
        .finish_agent_reasoning("a1", 2)
        .expect("the phase settles into a cell");
    assert_eq!(reasoning_lines(&thought, 80).len(), 1);
    app.apply_agent_event(
        "a1",
        &StreamEvent::ToolStart {
            name: "Bash".into(),
            args: "make build".into(),
            detail: None,
            arguments: None,
        },
    );
    for output in ["title\n", "\n", "row 1\nrow 2\n", "row 3\nrow 4\nrow 5\n"] {
        app.apply_agent_event("a1", &StreamEvent::ToolOutput(output.to_string()));
        let live = usize::from(preview_rows(&app, 80));
        let run = app.agent("a1").expect("listed");
        let call = run.tool_queue.front().expect("the call is running");
        let mut settled = call.clone();
        settled.status = ToolStatus::Ok;
        let settled = tool_lines(&settled, 80, &PathDisplay::VERBATIM).len();
        assert!(
            live <= settled,
            "the agent's running command stands {live} rows, taller than the {settled} it \
             would settle into after {output:?}"
        );
    }
}
