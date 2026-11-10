//! The thinking stream's three renderers: the live block in the strip, the
//! collapsed `Thought for …` line, and its Ctrl+O expansion.
//! See `docs/thinking-stream.md`.

use super::*;
use crate::app::Reasoning;
use crate::ui::live::preview_lines;
use crate::ui::reasoning::{live_reasoning_lines, reasoning_full_lines, reasoning_live_full_lines};
use crate::ui::theme::{
    SHIMMER_SWEEP, TOOL_PULSE_PERIOD, reasoning_label_color, reasoning_shimmer_base,
    reasoning_text_color, shimmer_highlight,
};
use crate::ui::wrap::cols;

fn thought(text: &str, secs: u64, tokens: usize) -> Reasoning {
    Reasoning {
        text: text.to_string(),
        secs,
        tokens,
        timestamp: String::new(),
    }
}

#[test]
fn the_collapsed_line_reads_thought_for_elapsed_tokens_and_the_expand_hint() {
    // The shape the feature was asked for, with the same humanizers the
    // status line and the turn summary use.
    let lines = reasoning_lines(&thought("…", 65, 1_500), 80);
    assert_eq!(lines.len(), 1, "one unwrapped line, like summary_lines");
    assert_eq!(
        plain(&lines[0]),
        "Thought for 1m 5s · 1.5k tokens (ctrl+o to expand)"
    );
}

#[test]
fn the_collapsed_line_hides_an_unknown_token_count() {
    // A backend that reported no usage and estimated nothing (an old rollout)
    // keeps the bare shape rather than claiming `· 0 tokens`.
    let lines = reasoning_lines(&thought("…", 8, 0), 80);
    assert_eq!(plain(&lines[0]), "Thought for 8s (ctrl+o to expand)");
}

#[test]
fn the_settled_line_carries_no_bullet() {
    // The `● Thinking…` header meant *something is happening*; nothing is any
    // more, so what is left is a fact about the turn — `Done for Ns`'s shape.
    let lines = reasoning_lines(&thought("…", 3, 12), 80);
    assert!(
        plain(&lines[0]).starts_with("Thought for"),
        "no bullet, no indent: {:?}",
        plain(&lines[0])
    );
}

#[test]
fn the_settled_line_is_dim_throughout() {
    // `Done for Ns`'s exact dress. A finished thought is a footnote about work
    // already done, so it settles into the transcript instead of competing
    // with the reply it sits above — the weight belongs to the live block.
    let lines = reasoning_lines(&thought("…", 3, 12), 80);
    let [span] = lines[0].spans.as_slice() else {
        panic!("one tone across the whole row, got {:?}", lines[0].spans);
    };
    assert_eq!(
        span.content,
        "Thought for 3s · 12 tokens (ctrl+o to expand)"
    );
    assert_eq!(span.style.fg, Some(reasoning_label_color()));
    assert!(!span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn the_transcript_expands_the_whole_chain_of_thought() {
    let reasoning = thought("first thought\n\nsecond thought", 40, 12);
    let lines = reasoning_full_lines(&reasoning, 40);
    let text: Vec<String> = lines.iter().map(plain).collect();
    assert_eq!(
        text[0], "Thought for 40s · 12 tokens",
        "the expansion drops the hint — this IS the expansion: {text:?}"
    );
    assert_eq!(
        text[1], "  ⎿  first thought",
        "the body hangs in the gutter"
    );
    assert!(
        text.iter().any(|l| l.contains("second thought")),
        "{text:?}"
    );
}

#[test]
fn the_expansion_drops_the_thoughts_blank_edges_but_keeps_its_paragraph_breaks() {
    // A model's reasoning often opens and closes on blank lines
    // (`<think>\n…\n\n</think>`). Those are not paragraph breaks: the opening
    // one pushed the `⎿` corner off the first row of thought, and the closing
    // ones stacked on the spacer the transcript puts after every item. The
    // blank line *between* two paragraphs stays.
    let reasoning = thought("\n\nfirst thought\n\nsecond thought\n\n \n", 40, 12);
    let text: Vec<String> = reasoning_full_lines(&reasoning, 40)
        .iter()
        .map(plain)
        .collect();
    assert_eq!(
        text,
        [
            "Thought for 40s · 12 tokens",
            "  ⎿  first thought",
            "",
            "     second thought",
        ]
    );
}

#[test]
fn an_open_phase_in_the_transcript_drops_its_blank_edges_too() {
    // The live expansion shares the body: a phase whose newest delta closed a
    // paragraph shows no blank row under it until the next paragraph starts.
    let text: Vec<String> = reasoning_live_full_lines("\nweighing options\n\n", 60)
        .iter()
        .map(plain)
        .collect();
    assert_eq!(text, ["● Thinking…", "  ⎿  weighing options"]);
}

#[test]
fn a_thought_closing_on_blank_lines_sits_one_blank_row_above_the_next_cell() {
    // The reported Ctrl+O screen: the thought ended on `\n\n`, and the pager
    // put three blank rows between its last line and the `● Bash(…)` under
    // it, where every other pair of cells has one.
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking(
        "I need to find the current NVDA stock price, so I'll try fetching it \
         from a finance data source to verify the number before answering.\n\n",
    );
    app.finish_reasoning(0);
    app.start_tool("Bash", "curl -s -m 20 https://example.com", None);
    app.end_tool("{}", true);
    let texts: Vec<String> = transcript_lines(&app, 87)
        .iter()
        .map(|l| plain(l).trim_end().to_string())
        .collect();
    let bash = texts
        .iter()
        .position(|t| t.starts_with("● Bash("))
        .expect("the call is in the transcript");
    assert_eq!(texts[bash - 1], "", "one spacer above the call: {texts:?}");
    assert!(
        texts[bash - 2].ends_with("before answering."),
        "and the thought's last line right above it: {texts:?}"
    );
}

#[test]
fn the_transcript_label_is_the_same_dim_line_with_no_background() {
    // A thought looks like the same thing wherever you meet it: the settled
    // line wears exactly what the inline one wears, minus the expand hint.
    let lines = reasoning_full_lines(&thought("hm", 3, 12), 40);
    let [span] = lines[0].spans.as_slice() else {
        panic!("one label span, got {:?}", lines[0].spans);
    };
    assert_eq!(span.content, "Thought for 3s · 12 tokens");
    assert_eq!(span.style.fg, Some(reasoning_label_color()));
    assert!(!span.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(span.style.bg, None, "no band, no padding to the width");
}

#[test]
fn the_live_block_is_one_row_the_label_then_the_newest_thought() {
    // One row, because one row is what it settles into: the `● Thinking…`
    // label and, beside it, the row of thought still being written
    // (docs/live-cells.md).
    let lines = live_reasoning_lines("weighing options", Duration::ZERO, 60);
    assert_eq!(lines.len(), 1, "{:?}", texts(&lines));
    assert_eq!(plain(&lines[0]), "● Thinking… weighing options");
}

fn texts(lines: &[Line<'static>]) -> Vec<String> {
    lines.iter().map(plain).collect()
}

/// Twenty one-row lines of thought, `line 1` … `line 20`.
fn twenty_lines() -> String {
    (1..=20)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_live_block_stands_exactly_as_tall_as_the_line_it_settles_into() {
    // The hand-off that moved the box: a block taller than its settled line
    // gave those rows up the moment the phase ended, and the box rose until
    // the next cell landed and pushed it back down. Whatever was thought, at
    // whatever width, the block stands on the settled line's one row.
    let long = "word ".repeat(400);
    let twenty = twenty_lines();
    for text in ["", "one", twenty.as_str(), "a\n\n\nb\n\n", long.as_str()] {
        for width in [1, 8, 13, 20, 60, 120] {
            assert_eq!(
                live_reasoning_lines(text, Duration::ZERO, width).len(),
                reasoning_lines(&thought(text, 3, 12), width).len(),
                "the live block and its settled line differ in height at width \
                 {width} over {:?}",
                text.chars().take(24).collect::<String>()
            );
        }
    }
}

#[test]
fn the_live_row_follows_the_newest_line_of_thought() {
    // A long think shows what just streamed, the way a running `bash` cell
    // tails its output — in the one row the block has.
    let lines = live_reasoning_lines(&twenty_lines(), Duration::ZERO, 60);
    assert_eq!(plain(&lines[0]), "● Thinking… line 20");
}

#[test]
fn a_wrapping_thought_shows_the_row_still_being_written() {
    // The line still streaming wraps in the room beside the label, and the
    // row shown is its last: it starts at a word and grows to the right, so
    // it reads as text arriving rather than as a marquee sliding left.
    let text = "- I should keep answers concise and formatted\n\
                The user just said \"Hi\" - this is not a request for any specific work, \
                no tasks, no files to read, no commands to";
    let lines = live_reasoning_lines(text, Duration::ZERO, 87);
    let row = plain(&lines[0]);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        row.trim_end(),
        "● Thinking… tasks, no files to read, no commands to",
        "the last row the line wraps to in the 75 columns beside the label"
    );
}

#[test]
fn the_live_row_never_outgrows_the_width() {
    // The row is one `Line`, so a width it overflowed would be clipped by
    // the paint — and a wide glyph cut in half. It is fitted here instead.
    let text = "alpha beta gamma delta epsilon zeta 表示幅の広い文字も折り返す";
    for width in [1, 5, 12, 14, 20, 21, 40, 87] {
        let lines = live_reasoning_lines(text, Duration::ZERO, width);
        assert_eq!(lines.len(), 1);
        let row = plain(&lines[0]);
        assert!(
            cols(&row) <= usize::from(width).max(cols("● Thinking…")),
            "width {width}: {row:?} is {} columns",
            cols(&row)
        );
    }
}

#[test]
fn the_live_row_skips_the_blank_lines_a_thought_ends_on() {
    // Reasoning is full of paragraph breaks, and a delta often ends on one:
    // the row keeps the last line that says something.
    let lines = live_reasoning_lines("one\n\n\ntwo\n\n \n", Duration::ZERO, 60);
    assert_eq!(plain(&lines[0]), "● Thinking… two");
}

#[test]
fn a_terminal_too_narrow_for_any_thought_keeps_the_label_alone() {
    // A couple of columns beside the label would show a syllable at a time.
    let lines = live_reasoning_lines("weighing options", Duration::ZERO, 16);
    assert_eq!(plain(&lines[0]), "● Thinking…");
}

#[test]
fn the_thought_beside_the_label_is_dim_and_italic() {
    // The cue that it is the model thinking aloud, not the label and not a
    // tool's output — the dress the Ctrl+O expansion gives the same text.
    let lines = live_reasoning_lines("weighing options", Duration::ZERO, 60);
    let thought = lines[0].spans.last().expect("the row has spans");
    assert_eq!(thought.content, "weighing options");
    assert_eq!(thought.style.fg, Some(reasoning_text_color()));
    assert!(thought.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn a_long_think_stands_one_row_tall_in_the_strip() {
    // What the live region spends on it: the one row, then the strip's gap
    // and the status line — however much the model has thought.
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking(&twenty_lines());
    let width = 60;
    let pv = preview_rows(&app, width);
    assert_eq!(pv, 1, "the block is one row");
    let h = live_height(&app.input, width, 24, true, pv, 0, 0, 0, 0, 0, 0);
    let mut buf = buffer(width, h);
    render_live(buf.area, &mut buf, &app);
    assert_eq!(row(&buf, 0, width).trim_end(), "● Thinking… line 20");
    assert!(
        row(&buf, 1, width).trim().is_empty(),
        "the strip's gap follows the block: {:?}",
        row(&buf, 1, width)
    );
}

#[test]
fn an_opening_phase_with_no_text_yet_is_just_the_label() {
    // ThinkingStart arrives before the first delta — the block says the model
    // is thinking, and the row is already the height it will keep.
    let lines = live_reasoning_lines("", Duration::ZERO, 60);
    assert_eq!(lines.len(), 1);
    assert_eq!(plain(&lines[0]), "● Thinking…");
}

/// The label's own spans on a live row: after the bullet, before the thought.
fn label_spans(line: &Line<'static>) -> Vec<(String, Option<Color>)> {
    line.spans
        .iter()
        .skip(1)
        .take("Thinking…".chars().count())
        .map(|s| (s.content.to_string(), s.style.fg))
        .collect()
}

#[test]
fn the_live_label_shimmers_at_the_frame_pulse() {
    // The status line's own white sweep, on `Thinking…` — one span per char,
    // each a different point of the wave, and the wave moves between frames.
    let at = |pulse| label_spans(&live_reasoning_lines("x", pulse, 60)[0]);
    let early = at(Duration::ZERO);
    assert_eq!(
        early.len(),
        "Thinking…".chars().count(),
        "one shimmer span per char"
    );
    assert_eq!(
        early.iter().map(|(c, _)| c.as_str()).collect::<String>(),
        "Thinking…"
    );
    assert_ne!(
        early,
        at(SHIMMER_SWEEP / 3),
        "the sweep advances with the frame clock"
    );
}

#[test]
fn the_live_label_rests_at_bold_white_not_codexs_grey() {
    // Between crests — most of the sweep — the header must still read as a
    // header. Codex's grey base is right for the status metric below it and
    // wrong here: at rest it would be indistinguishable from the dim body.
    let resting: Vec<Color> = (0..40)
        .map(|i| {
            live_reasoning_lines("x", SHIMMER_SWEEP * i / 40, 60)[0].spans[1]
                .style
                .fg
        })
        .map(|fg| fg.expect("the label is coloured"))
        .collect();
    assert!(
        resting.contains(&reasoning_shimmer_base()),
        "the wave rests at the near-white floor: {resting:?}"
    );
    assert!(
        !resting.contains(&reasoning_text_color()),
        "and never at the dim the thought under it wears: {resting:?}"
    );
    let brightest = resting
        .iter()
        .map(|fg| {
            let Color::Rgb(r, _, _) = fg else {
                panic!("expected an RGB colour, got {fg:?}")
            };
            assert!(
                *r >= rgb_of(reasoning_shimmer_base()).0,
                "never dimmer than the floor"
            );
            *r
        })
        .max()
        .expect("sampled the sweep");
    // The crest blends SHIMMER_MAX_BLEND (0.9) of the way from the floor to
    // the text colour — whatever the theme's text colour is.
    let floor = rgb_of(reasoning_shimmer_base()).0;
    let crest = rgb_of(shimmer_highlight()).0;
    let reach = floor + ((crest - floor) as f32 * 0.85) as u8;
    assert!(
        brightest >= reach,
        "and the crest does reach the text — a wave nobody sees is not a wave \
         (brightest sampled: {brightest:#04x}, expected at least {reach:#04x})"
    );
}

#[test]
fn the_transcript_header_never_shimmers() {
    // The overlay's cache signature is clock-free, so its header must render
    // at rest — one plain label span, not a per-char wave.
    let lines = reasoning_live_full_lines("x", 60);
    assert_eq!(lines[0].spans.len(), 2, "bullet + one plain label span");
    assert_eq!(lines[0].spans[1].content, "Thinking…");
}

#[test]
fn the_live_bullet_blinks_but_the_committed_one_never_does() {
    // The blink is live-only, like a running tool's (docs/tool-pulse.md): a
    // scrollback commit must never freeze a hidden frame of the animation.
    let shown = live_reasoning_lines("x", Duration::ZERO, 60)[0].spans[0]
        .content
        .to_string();
    let hidden = live_reasoning_lines("x", TOOL_PULSE_PERIOD / 2, 60)[0].spans[0]
        .content
        .to_string();
    assert_eq!(shown, "● ", "the live header shows its bullet");
    assert_eq!(hidden, "  ", "…and hides it half a period on");
    assert_eq!(
        reasoning_lines(&thought("x", 1, 1), 60)[0].spans[0]
            .style
            .fg,
        Some(reasoning_label_color()),
        "the committed line is a fixed colour — no frame of the pulse or the \
         shimmer can be frozen into scrollback"
    );
}

#[test]
fn a_live_phase_previews_in_the_strip() {
    // The strip's preview slot shows the block, and `preview_rows` sizes
    // exactly what `preview_lines` draws (the strip's debug_assert).
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking("a\nb");
    let width = 60;
    let pv = preview_rows(&app, width);
    assert_eq!(pv, 1, "the block's one row");
    let h = live_height(&app.input, width, 24, true, pv, 0, 0, 0, 0, 0, 0);
    let mut buf = buffer(width, h);
    render_live(buf.area, &mut buf, &app);
    assert_eq!(row(&buf, 0, width).trim_end(), "● Thinking… b");
}

#[test]
fn a_running_tool_still_wins_the_preview_slot() {
    // Thinking always precedes the round's calls, so this never really
    // collides — but what is executing is what the user is waiting on.
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking("thought");
    app.start_tool("bash", "ls", None);
    let lines = preview_lines(&app, 60, None, preview_rows(&app, 60));
    assert!(
        plain(&lines[0]).contains("bash"),
        "the running call previews: {:?}",
        plain(&lines[0])
    );
}

#[test]
fn a_settled_thought_repaints_in_the_conversation_and_the_transcript() {
    let mut app = App::new();
    app.begin_stream();
    app.begin_reasoning();
    app.push_thinking("the plan");
    app.finish_reasoning(7);
    let inline: Vec<String> = conversation_lines(&app.history, 60, &PathDisplay::VERBATIM, true)
        .iter()
        .map(plain)
        .collect();
    assert!(
        inline
            .iter()
            .any(|l| l == "Thought for 7s · 2 tokens (ctrl+o to expand)"),
        "collapsed inline: {inline:?}"
    );
    assert!(
        !inline.iter().any(|l| l.contains("the plan")),
        "the chain-of-thought never commits to scrollback: {inline:?}"
    );
    let full: Vec<String> = transcript_lines(&app, 60).iter().map(plain).collect();
    assert!(
        full.iter().any(|l| l.contains("the plan")),
        "and expands in Ctrl+O: {full:?}"
    );
}
