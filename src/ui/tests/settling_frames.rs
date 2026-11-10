//! Whole turns played frame by frame the way the boundary paints them, with
//! cells settling in place (`docs/settling.md`).
//!
//! [`Sim`] is a model of `tui`'s half of the feature over the real pure core:
//! the event arms that commit a cell (`tui::stream`), the per-frame sync
//! (`tui::settle`), and the viewport's bookkeeping (`term::InlineViewport` —
//! queued lines are written above the region, which is content-anchored and
//! scrolls only when it would run past the screen). The boundary itself is
//! smoke-covered (Phase 153); what this pins is the *logic* it runs.
//!
//! Four properties are checked on every turn, the first three on every
//! frame of it:
//!
//! 1. **The box never moves up** while the turn is in flight.
//! 2. **No row is padding**: the strip never shows two blank rows in a row,
//!    and never ends on one — the one blank row above the status line is the
//!    strip's own gap.
//! 3. **A settled line stands alone**: a thought's `Thought for …` line is
//!    never on screen over the thought it stands for.
//! 4. **Scrollback is the conversation**: what was committed, in the order it
//!    was committed, is what a rebuild from history would paint.

use super::*;
use crate::ui::live::preview_lines;
use crate::ui::settling::{
    LivePicture, SettleGeometry, command_picture, group_picture, settle_frame, settles_in_place,
    settling_cell, settling_holds, thought_picture,
};

/// One painted frame.
#[derive(Debug, Clone)]
struct Shot {
    /// The screen row just below the region.
    bottom: u16,
    /// The preview slot's rows, as text.
    preview: Vec<String>,
    /// What the frame was painted after.
    after: String,
}

struct Sim {
    app: App,
    width: u16,
    screen: u16,
    /// The region's top row, and where it was last painted to.
    top: u16,
    painted_bottom: u16,
    /// Lines queued for scrollback, and the batches held back from it.
    pending: Vec<String>,
    held: Vec<Vec<String>>,
    /// Everything written above the region so far.
    scrollback: Vec<String>,
    render: StreamRender,
    shots: Vec<Shot>,
}

impl Sim {
    /// An idle session whose box sits `above_bottom` rows above the bottom of
    /// a `screen`-row terminal (0 = flush with it, the usual place after the
    /// first screenful).
    fn new(width: u16, screen: u16, above_bottom: u16) -> Self {
        let mut sim = Self {
            app: App::new(),
            width,
            screen,
            top: 0,
            painted_bottom: 0,
            pending: Vec::new(),
            held: Vec::new(),
            scrollback: Vec::new(),
            render: StreamRender::new(),
            shots: Vec::new(),
        };
        let height = sim.height();
        sim.top = screen - above_bottom - height;
        sim.painted_bottom = sim.top + height;
        sim
    }

    /// The region's height for the conversation view — `tui::view`'s
    /// `live_region_height`, composer path.
    fn height(&self) -> u16 {
        let app = &self.app;
        let band = band_rows(app, self.width);
        live_height(
            &app.input,
            self.width,
            self.screen,
            strip_has_status(app),
            fitted_preview_rows(app, self.width, self.screen),
            hang_rows(app, self.width),
            queued_rows(app, self.width),
            toast_rows(app),
            band,
            footer_rows(app, band),
            agent_list_rows(app),
        )
    }

    // ----- the viewport ---------------------------------------------------

    /// `InlineViewport::insert_before`: queue lines for scrollback. Whatever
    /// was held is older than what commits now, so it goes first.
    fn commit(&mut self, lines: &[Line<'static>]) {
        if lines.is_empty() {
            return;
        }
        for batch in self.held.drain(..) {
            self.pending.extend(batch);
        }
        self.pending.extend(lines.iter().map(plain));
    }

    /// `Session::commit_cell`: a cell that just ended either settles in
    /// place — its lines held back, its picture in the strip — or commits.
    fn commit_cell(&mut self, lines: Vec<Line<'static>>, picture: Option<LivePicture>) {
        let cell = settling_cell(picture, &lines);
        let mut batch = lines;
        batch.push(Line::default());
        if settles_in_place(&self.app, &cell, self.width, self.screen) {
            self.held.push(batch.iter().map(plain).collect());
            self.app.push_settling(cell);
        } else {
            self.commit(&batch);
        }
    }

    /// One draw tick: the sync, then the paint.
    fn frame(&mut self, after: &str) -> &Shot {
        // The reply's frontier, injected like `Session::stream_preview_lines`.
        let max_rows = stream_preview_max_rows(&self.app, self.width, self.screen);
        let preview = match self.app.streaming_text() {
            Some(text) if !text.is_empty() && self.app.current_tool().is_none() => {
                Some(self.render.preview(text, self.width, max_rows))
            }
            _ => None,
        };
        self.app
            .set_stream_preview_rows(preview.as_ref().map_or(1, |p| p.len() as u16));

        // `Session::sync_settling`.
        if self.app.settling().len() != self.held.len() {
            // A commit that was not a cell's released every held batch.
            for batch in self.held.drain(..) {
                self.pending.extend(batch);
            }
            self.app.clear_settling();
        }
        if !self.app.settling().is_empty() {
            let geometry = SettleGeometry {
                painted_bottom: self.painted_bottom,
                view_top: self.top,
                pending_rows: self.pending.len(),
                height: self.height(),
                holds: settling_holds(&self.app, self.width, self.screen),
            };
            let released = settle_frame(&mut self.app, &geometry);
            for batch in self.held.drain(..released) {
                self.pending.extend(batch);
            }
        }
        assert_eq!(
            self.app.settling().len(),
            self.held.len(),
            "a settling cell and its held lines go together"
        );

        // `InlineViewport::paint_live`: flush, then re-pin.
        let height = self.height();
        let queued = u16::try_from(self.pending.len()).unwrap();
        self.scrollback.append(&mut self.pending);
        self.top = (self.top + queued).min(self.screen - height);
        self.painted_bottom = self.top + height;

        let rows = fitted_preview_rows(&self.app, self.width, self.screen);
        let lines = preview_lines(&self.app, self.width, preview.as_deref(), rows);
        assert_eq!(
            lines.len(),
            usize::from(rows),
            "the rows reserved and the rows drawn"
        );
        self.shots.push(Shot {
            bottom: self.painted_bottom,
            preview: lines.iter().map(plain).collect(),
            after: after.to_string(),
        });
        self.shots.last().unwrap()
    }

    // ----- the event arms (tui::stream) -----------------------------------

    fn submit(&mut self, text: &str) {
        self.app.record_user_message(text);
        self.commit(&message_lines(Role::User, text, self.width));
        self.commit(&[Line::default()]);
        self.app.begin_stream();
        self.frame("submit");
    }

    fn flush_segment(&mut self) {
        if self
            .app
            .streaming_text()
            .is_some_and(|text| !text.trim().is_empty())
        {
            self.close_group();
        }
        if let Some(segment) = self.app.flush_streaming_segment() {
            let lines = self.render.finish(&segment, self.width);
            self.commit(&lines);
            self.commit(&[Line::default()]);
        }
        self.render.reset();
    }

    fn close_group(&mut self) {
        let picture = group_picture(&self.app, self.width);
        if let Some(range) = self.app.close_tool_group() {
            let lines = tool_group_lines(
                &self.app.history[range],
                self.width,
                self.app.path_display(),
            );
            self.commit_cell(lines, picture);
        }
    }

    fn close_group_unless_the_front_reads(&mut self) {
        let joins =
            self.app.tool_queue().front().is_some_and(|call| {
                crate::app::group_kind(call, self.app.path_display()).is_some()
            });
        if !joins {
            self.close_group();
        }
    }

    /// `ThinkingStart` and the phase's deltas, a frame after each.
    fn think(&mut self, lines: &[&str]) {
        self.flush_segment();
        self.app.begin_reasoning();
        for line in lines {
            self.app.push_thinking(&format!("{line}\n"));
            self.frame("thinking");
        }
    }

    /// `ThinkingEnd` — no frame of its own: the backend sends it with the
    /// first piece of whatever follows.
    fn thinking_end(&mut self) {
        let picture = thought_picture(&self.app, self.width, 1);
        self.flush_segment();
        let Some(reasoning) = self.app.finish_reasoning(1) else {
            return;
        };
        if self.app.tool_group_range().is_some() {
            return;
        }
        self.commit_cell(reasoning_lines(&reasoning, self.width), picture);
    }

    /// The round's batch is announced: every call a `⎿ Waiting…` cell.
    fn batch(&mut self, calls: &[(&str, &str)]) {
        self.flush_segment();
        let items: Vec<crate::stream::ToolCallSummary> = calls
            .iter()
            .map(|(name, args)| crate::stream::ToolCallSummary {
                name: (*name).to_string(),
                args: (*args).to_string(),
            })
            .collect();
        self.app.start_tool_batch(&items);
        self.close_group_unless_the_front_reads();
        self.frame("batch");
    }

    fn start(&mut self, name: &str, args: &str) {
        self.flush_segment();
        self.app.start_tool(name, args, None);
        self.close_group_unless_the_front_reads();
        self.app.set_command_elapsed(Some(Duration::from_secs(1)));
        self.frame("start");
    }

    fn output(&mut self, lines: &[&str]) {
        for line in lines {
            self.app.push_tool_output(&format!("{line}\n"));
            self.frame("output");
        }
    }

    /// `ToolEnd`: the call resolves and its cell settles or commits.
    fn end(&mut self, output: &str, ok: bool) {
        self.resolve(output, ok);
        self.frame("end");
    }

    /// [`Sim::end`] without its frame, for an event that lands with it.
    fn resolve(&mut self, output: &str, ok: bool) {
        let picture = command_picture(&self.app, ok, self.width);
        let group = group_picture(&self.app, self.width);
        self.app.end_tool(output, ok).expect("a call was running");
        match self.app.place_resolved_tool() {
            crate::app::ToolPlacement::Held => {}
            crate::app::ToolPlacement::Commit { closed } => {
                if let Some(range) = closed {
                    let lines = tool_group_lines(
                        &self.app.history[range],
                        self.width,
                        self.app.path_display(),
                    );
                    self.commit_cell(lines, group);
                }
                if let Some(lines) = tool_commit_lines(
                    &self.app.history,
                    self.app.tool_queue(),
                    self.width,
                    self.app.path_display(),
                ) {
                    self.commit_cell(lines, picture);
                }
            }
        }
    }

    /// Enter mid-turn: the message waits above the box until the turn's
    /// next round boundary takes it (`docs/queue.md`).
    fn queue_steered(&mut self, text: &str) {
        self.app.steered.push_back(text.to_string());
        self.frame("queued");
    }

    /// `Steered`: the round boundary took the message and it becomes a user
    /// bubble. No frame of its own — the backend sends it straight after the
    /// round's results.
    fn steered(&mut self, text: &str) {
        self.flush_segment();
        self.close_group();
        self.app.deliver_steered(text);
        // A bubble is a cell like any other: it waits behind one that is
        // still settling rather than pushing it out early.
        self.commit_cell(message_lines(Role::User, text, self.width), None);
    }

    /// A piece of reply text.
    fn chunk(&mut self, text: &str) {
        self.app.push_chunk(text);
        if self
            .app
            .streaming_text()
            .is_some_and(|text| !text.trim().is_empty())
        {
            self.close_group();
        }
        if let Some(text) = self.app.streaming_text() {
            let lines = self.render.commit(text, self.width);
            self.commit(&lines);
        }
        self.frame("chunk");
    }

    fn say(&mut self, text: &str) {
        for word in text.split_inclusive(' ') {
            self.chunk(word);
        }
    }

    /// `StreamDone`.
    fn done(&mut self) {
        self.close_group();
        let text = self.app.finish_stream();
        let summary = self.app.take_turn_summary(3);
        if let Some(text) = text {
            let lines = self.render.finish(&text, self.width);
            self.commit(&lines);
            self.commit(&[Line::default()]);
        }
        if let Some(summary) = summary {
            self.commit(&summary_lines(&summary, self.width));
            self.commit(&[Line::default()]);
            self.app.record_turn_summary(summary);
        }
        self.render.reset();
        self.frame("done");
    }

    // ----- the four properties --------------------------------------------

    #[track_caller]
    fn check(&self) {
        // 1. The box never moves up.
        for pair in self.shots.windows(2) {
            assert!(
                pair[1].bottom >= pair[0].bottom,
                "the box moved up {} row(s) after `{}` (from row {} to row {}):\n{}\n  ↓\n{}",
                pair[0].bottom - pair[1].bottom,
                pair[1].after,
                pair[0].bottom,
                pair[1].bottom,
                pair[0].preview.join("\n"),
                pair[1].preview.join("\n"),
            );
        }
        // 2. No row is padding.
        for shot in &self.shots {
            assert!(
                shot.preview.last().is_none_or(|row| !row.trim().is_empty()),
                "the strip ends on a blank row after `{}`: {:#?}",
                shot.after,
                shot.preview
            );
            assert!(
                !shot
                    .preview
                    .windows(2)
                    .any(|rows| rows[0].trim().is_empty() && rows[1].trim().is_empty()),
                "two blank rows in a row after `{}`: {:#?}",
                shot.after,
                shot.preview
            );
        }
        // 3. A settled line stands alone: a thought's `Thought for …` is
        // never drawn over the thought it stands for.
        for shot in &self.shots {
            for (index, row) in shot.preview.iter().enumerate() {
                if !row.starts_with("Thought for ") {
                    continue;
                }
                assert!(
                    shot.preview
                        .get(index + 1)
                        .is_none_or(|next| next.trim().is_empty()),
                    "a settled thought's line over its own thought after `{}`: {:#?}",
                    shot.after,
                    shot.preview
                );
            }
        }
        // 4. Scrollback is the conversation a rebuild would paint.
        assert!(self.held.is_empty(), "the turn left lines held back");
        assert!(self.app.settling().is_empty());
        let rebuilt: Vec<String> = conversation_lines(
            &self.app.history,
            self.width,
            self.app.path_display(),
            self.app.settings().verbose,
        )
        .iter()
        .map(plain)
        .collect();
        assert_eq!(self.scrollback, rebuilt, "scrollback and history disagree");
    }
}

/// Every placement the box can start a turn at on a 30-row screen: flush
/// with the bottom, and floating above it.
const PLACES: [u16; 3] = [0, 6, 14];

#[test]
fn a_thought_handing_over_to_a_tool_call_never_moves_the_box() {
    // The reported turn: think, load a skill, think, run a command, answer.
    // The thought used to collapse to its line the moment the model started
    // writing the call, and the box jumped up three rows until the call's
    // cell landed.
    for above in PLACES {
        let mut sim = Sim::new(100, 30, above);
        sim.submit("who is the champion");
        sim.think(&[
            "The user wants the champion.",
            "Load the skill first.",
            "Then search.",
        ]);
        let thinking = sim.shots.last().unwrap().preview.clone();
        sim.thinking_end();
        sim.frame("the call is being written");
        let held = sim.shots.last().unwrap().preview.clone();
        assert_eq!(
            held,
            [
                "● Thinking…",
                "  ⎿  The user wants the champion.",
                "     Load the skill first.",
                "     Then search.",
            ],
            "the block stays as it was"
        );
        assert_eq!(held, thinking, "the phase's end changes nothing on screen");
        assert!(
            !sim.scrollback
                .iter()
                .any(|row| row.starts_with("Thought for")),
            "its line waits for the rows to be taken"
        );
        sim.batch(&[("Skill", "web-search")]);
        let landed = sim.shots.last().unwrap().preview.clone();
        assert_eq!(landed[0], "● Skill(web-search)", "{landed:#?}");
        assert!(
            sim.scrollback
                .iter()
                .any(|row| row.starts_with("Thought for 1s · ")),
            "the line commits the frame the call's cell lands"
        );
        sim.start("Skill", "web-search");
        sim.end("Successfully loaded skill", true);
        sim.think(&["The skill is loaded.", "Run the search."]);
        sim.thinking_end();
        sim.batch(&[("Bash", "python3 search.py")]);
        sim.start("Bash", "python3 search.py");
        sim.output(&["one", "two", "three", "four", "five", "six"]);
        sim.end("Exit code: 0\none\ntwo\nthree\nfour\nfive\nsix\n", true);
        sim.frame("the next request is out");
        sim.think(&["I have it."]);
        sim.thinking_end();
        sim.say("The champion is whoever the search says. ");
        sim.done();
        sim.check();
    }
}

#[test]
fn a_command_keeps_its_rows_until_the_next_cell_lands() {
    // Between rounds the finished command stands where it ran — its bullet
    // green, its clock stopped — and the next round's first cell takes the
    // row its committed form does not need.
    let mut sim = Sim::new(100, 30, 0);
    sim.submit("run it");
    sim.batch(&[("Bash", "make test")]);
    sim.start("Bash", "make test");
    sim.output(&["a", "b", "c", "d", "e", "f", "g"]);
    let running = sim.shots.last().unwrap().clone();
    sim.end("Exit code: 0\na\nb\nc\nd\ne\nf\ng\n", true);
    let settled = sim.shots.last().unwrap().clone();
    assert_eq!(settled.bottom, running.bottom, "the box holds still");
    assert_eq!(
        settled.preview,
        [
            "● Bash(make test)",
            "  ⎿  d",
            "     e",
            "     f",
            "     g",
            "     +3 lines (1s)",
        ],
        "the cell it ran in, stopped"
    );
    assert!(
        !sim.scrollback.iter().any(|row| row.contains("make test")),
        "its committed form waits for the row to be taken"
    );
    // The next round thinks: the block's rows take the cell's spare one.
    sim.think(&["Now the next step."]);
    let after = sim.shots.last().unwrap().clone();
    assert!(
        sim.scrollback.iter().any(|row| row == "● Bash(make test)"),
        "the cell committed the frame the next one landed"
    );
    assert_eq!(after.preview[0], "● Thinking…");
    sim.thinking_end();
    sim.say("Done. ");
    sim.done();
    sim.check();
}

#[test]
fn a_command_cut_at_a_blank_line_gives_its_rows_up_one_by_one() {
    // The settled peek is the output's first block: `first`, then the hint —
    // three rows shorter than the cell that ran. The reply's first row takes
    // two of them (its row and the blank above it) and the cell keeps the
    // third until the reply's first line commits.
    for above in PLACES {
        let mut sim = Sim::new(100, 30, above);
        sim.submit("run it");
        sim.batch(&[("Bash", "printf demo")]);
        sim.start("Bash", "printf demo");
        sim.output(&["first", "", "second", "third", "fourth", "fifth"]);
        sim.end(
            "Exit code: 0\nfirst\n\nsecond\nthird\nfourth\nfifth\n",
            true,
        );
        sim.frame("the next request is out");
        sim.say("The command printed six lines, and that is the whole of the answer. ");
        sim.say("A second sentence makes the reply wrap onto another row of the terminal. ");
        sim.done();
        sim.check();
    }
}

#[test]
fn a_thought_handing_over_to_the_reply_never_moves_the_box() {
    // Thinking, then the answer: the reply's first row is shorter than the
    // thought's tail, so the block keeps one row of thought under its header
    // until the reply's first line commits — the hop the first fix for this
    // left in.
    for above in PLACES {
        let mut sim = Sim::new(60, 30, above);
        sim.submit("answer me");
        sim.think(&[
            "First consideration.",
            "Second consideration.",
            "Third consideration.",
        ]);
        sim.thinking_end();
        sim.chunk("The ");
        let first = sim.shots.last().unwrap().clone();
        assert_eq!(
            first.preview,
            ["● Thinking…", "  ⎿  Third consideration.", "", "● The",],
            "the header at rest, one row of thought, a blank, the frontier"
        );
        assert_eq!(
            sim.app.lingering_thought(),
            None,
            "the answer has started: the status line no longer says thinking"
        );
        sim.say("answer is long enough to wrap across several rows of a narrow terminal, ");
        sim.say("so its lines commit one after another while the rest of it streams in. ");
        sim.done();
        sim.check();
    }
}

#[test]
fn a_parallel_batch_never_moves_the_box() {
    // Three commands in one round: each ends a row taller than it commits,
    // while the next sits under it as `⎿ Waiting…` — the box used to jump up
    // a row at every one, and back as the next printed.
    for above in PLACES {
        let mut sim = Sim::new(100, 30, above);
        sim.submit("ping them");
        sim.think(&["Three hosts."]);
        sim.thinking_end();
        sim.batch(&[("Bash", "ping a"), ("Bash", "ping b"), ("Bash", "ping c")]);
        for host in ["a", "b", "c"] {
            sim.start("Bash", &format!("ping {host}"));
            sim.output(&["reply 1", "reply 2", "reply 3", "reply 4", "reply 5"]);
            sim.end(
                "Exit code: 0\nreply 1\nreply 2\nreply 3\nreply 4\nreply 5\n",
                true,
            );
        }
        sim.frame("the next request is out");
        sim.say("All three answered. ");
        sim.done();
        sim.check();
    }
}

#[test]
fn a_silent_command_behind_a_settling_one_waits_its_turn() {
    // The second command prints nothing: its cell is as tall settled as it
    // ran, but the one ahead of it still holds a row — scrollback takes them
    // in order, so both wait for the next round.
    let mut sim = Sim::new(100, 30, 0);
    sim.submit("run both");
    sim.batch(&[("Bash", "make build"), ("Bash", "make install")]);
    sim.start("Bash", "make build");
    sim.output(&["compiling", "linking"]);
    sim.end("Exit code: 0\ncompiling\nlinking\n", true);
    sim.start("Bash", "make install");
    sim.end("Exit code: 0\n", true);
    let waiting = sim.shots.last().unwrap().clone();
    assert_eq!(
        waiting.preview,
        [
            "● Bash(make build)",
            "  ⎿  compiling",
            "     linking",
            "     (1s)",
            "",
            "● Bash(make install)",
            "  ⎿  (no output)",
        ]
    );
    sim.say("Built and installed. ");
    let build = sim
        .scrollback
        .iter()
        .position(|row| row == "● Bash(make build)")
        .expect("the first cell committed");
    let install = sim
        .scrollback
        .iter()
        .position(|row| row == "● Bash(make install)")
        .expect("the second cell committed");
    assert!(build < install, "in the order they ran");
    sim.done();
    sim.check();
}

#[test]
fn a_read_group_closing_under_the_next_call_never_moves_the_box() {
    // The collapsed group's cell is its line and the value it shows; it
    // commits as the line alone when a call that is no read starts.
    for above in PLACES {
        let mut sim = Sim::new(100, 30, above);
        sim.submit("fix it");
        sim.batch(&[
            ("Read", "about.py"),
            ("Edit", "about.py"),
            ("Bash", "python3 about.py"),
        ]);
        sim.start("Read", "about.py");
        sim.end("1 x", true);
        sim.start("Edit", "about.py");
        sim.end("Updated about.py (+1 -1)\n1 -x\n1 +y\n", true);
        sim.start("Bash", "python3 about.py");
        sim.output(&["y"]);
        sim.end("Exit code: 0\ny\n", true);
        sim.frame("the next request is out");
        sim.say("Fixed. ");
        sim.done();
        sim.check();
    }
}

#[test]
fn a_turn_that_ends_on_a_settling_cell_commits_it() {
    // No reply follows the command: the turn's end is what lets it go.
    let mut sim = Sim::new(100, 30, 0);
    sim.submit("just run it");
    sim.batch(&[("Bash", "make")]);
    sim.start("Bash", "make");
    sim.output(&["ok"]);
    sim.end("Exit code: 0\nok\n", true);
    assert_eq!(sim.app.settling().len(), 1);
    sim.done();
    assert!(sim.app.settling().is_empty());
    assert!(sim.scrollback.iter().any(|row| row == "● Bash(make)"));
    // Scrollback order and content, less the box check: a turn's end is a
    // settle, and here nothing took the cell's spare row.
    let rebuilt: Vec<String> = conversation_lines(
        &sim.app.history,
        sim.width,
        sim.app.path_display(),
        sim.app.settings().verbose,
    )
    .iter()
    .map(plain)
    .collect();
    assert_eq!(sim.scrollback, rebuilt);
}

#[test]
fn a_terminal_too_short_to_hold_a_cell_lets_it_go() {
    // On a terminal with no room for the strip nothing is held: the cells
    // commit as they end, and the strip's own flow owns the rest
    // (docs/strip-flow.md).
    let mut sim = Sim::new(80, 8, 0);
    sim.submit("run it");
    sim.batch(&[("Bash", "make test")]);
    sim.start("Bash", "make test");
    sim.output(&["a", "b", "c", "d", "e"]);
    sim.end("Exit code: 0\na\nb\nc\nd\ne\n", true);
    assert!(sim.app.settling().is_empty(), "nothing held");
    assert!(sim.scrollback.iter().any(|row| row == "● Bash(make test)"));
}

#[test]
fn a_message_delivered_as_a_command_ends_never_moves_the_box() {
    // Enter mid-turn hands a message to the running turn, and the round
    // boundary right after the command's result takes it. The command's
    // cell and the pending row both leave the strip in that one frame, and
    // the bubble that replaces them is shorter than the two together: the
    // box used to jump up, and back down when the next round began.
    for above in PLACES {
        let mut sim = Sim::new(100, 30, above);
        sim.submit("run it");
        sim.batch(&[("Bash", "printf demo")]);
        sim.start("Bash", "printf demo");
        sim.output(&["first", "", "second", "third", "fourth", "fifth"]);
        sim.queue_steered("also name the runner-up");
        let queued = sim.shots.last().unwrap().clone();
        sim.resolve(
            "Exit code: 0\nfirst\n\nsecond\nthird\nfourth\nfifth\n",
            true,
        );
        sim.steered("also name the runner-up");
        let delivered = sim.frame("delivered").clone();
        assert_eq!(delivered.bottom, queued.bottom, "the box holds still");
        assert_eq!(
            delivered.preview.first().map(String::as_str),
            Some("● Bash(printf demo)"),
            "the command still stands on the rows it needs: {:#?}",
            delivered.preview
        );
        // The bubble's row is padded to the width — its ground fills it.
        assert_eq!(
            delivered.preview.last().map(|row| row.trim_end()),
            Some("❯ also name the runner-up"),
            "over the bubble, which waits behind it: scrollback is in order"
        );
        sim.think(&["Now the runner-up."]);
        sim.thinking_end();
        sim.say("Both names, as asked. ");
        sim.done();
        sim.check();
    }
}
