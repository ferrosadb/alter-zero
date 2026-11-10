//! Settling cells in the strip: the picture a live cell leaves when it ends,
//! how it is drawn at the rows it keeps, and how many rows each cell keeps
//! this frame. See `docs/settling.md`.
//!
//! The live region is content-anchored (CLAUDE.md invariant 3): a frame that
//! sheds more rows from the strip than it commits to scrollback pulls the box
//! **up** by the difference, and the next content pushes it back down. A live
//! cell is taller than the line it commits by design — a thinking block's
//! tail, a running command's tail and clock, a read group's hint row — so
//! every hand-off between two cells used to be that hop: the reported flicker
//! between a thought and its tool call, and between one command and the next.
//!
//! So a live cell that ends does not collapse at once. It **keeps the rows
//! it stood on** and gives them up one for one as whatever comes next needs
//! them — committing the frame it is down to its committed height. No row is
//! ever blank padding, and the box never moves up while a turn is in flight.
//!
//! What it looks like meanwhile was learned the hard way. A thought's
//! `Thought for …` line drawn over the thought it stands for read as a flash
//! of stale thought, and the same rows left empty read as newlines reserved
//! for whatever came next. So a **thought** and a **read/search group** keep
//! their live cell *exactly as it was* — header, blink and all — and their
//! settled line is only ever drawn alone, the frame the next cell lands;
//! nothing on screen changes when the phase ends. A **command** does change:
//! its bullet stops in the outcome's colour and its clock stops, because
//! that it finished is news, and its output is as true as it was.
//!
//! Three pieces, all pure:
//!
//! - the **pictures** ([`thought_picture`], [`command_picture`],
//!   [`group_picture`]) — what a live cell leaves behind, built from the very
//!   walk its live renderer draws from, so the rows agree by construction;
//! - the **renderer** ([`settling_lines`]) — a cell at the rows it keeps;
//! - the **plan** ([`settle_plan`], [`settle_frame`]) — how many rows each
//!   cell keeps this frame, from where the region was last painted.
//!
//! The boundary (`tui::settle`) only holds the committed lines back until
//! the plan lets a cell go.

use std::borrow::Cow;

use super::reasoning::{
    body_row, live_thinking_header, resting_thinking_header, thought_tail_rows,
};
use super::theme::tool_running_color;
use super::tool::{
    bullet_span, command_tail, hidden_lead, is_command_tool, output_row, result_row,
    tool_header_lines, tool_status_color,
};
use super::*;
use crate::app::{Settling, SettlingBody, SettlingDress, SettlingHead, TurnStatus, View};

/// What a live cell leaves in the strip when it ends: the header it wore and
/// the body it stood on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePicture {
    head: SettlingHead,
    body: SettlingBody,
}

/// The open thought whose live block the strip is showing for the
/// conversation on screen — `None` when no phase is open, or something else
/// holds the preview slot. Mirrors [`preview_lines`](super::live::preview_lines)'
/// own order: a running or queued call, a `write` being generated and an open
/// read/search group each outrank the block (`docs/thinking-stream.md`).
fn shown_thought(app: &App, width: u16) -> Option<&str> {
    if let Some(run) = app.viewed_agent() {
        let free = run.tool_queue.is_empty() && run.drafts().previews().next().is_none();
        return run.reasoning().filter(|_| free);
    }
    let free = app.agent_group().is_none()
        && app.tool_queue().is_empty()
        && app.tool_drafts().previews().next().is_none()
        && super::live::open_group_lines(app, width).is_empty();
    app.reasoning().filter(|_| free)
}

/// The picture the open thinking phase leaves when it ends: its live block,
/// unchanged — the `● Thinking…` header over the rows of thought it is
/// showing. `secs` is how long the phase ran, which the status line goes on
/// saying beside it ([`App::lingering_thought`]). `None` when the strip is not
/// showing the block, or the block has no row of thought under its header —
/// then nothing stands where the committed line will not.
///
/// Take it **before** the phase is recorded (`App::finish_reasoning`), which
/// closes the buffer the rows are read from.
#[must_use]
pub fn thought_picture(app: &App, width: u16, secs: u64) -> Option<LivePicture> {
    let rows = thought_tail_rows(shown_thought(app, width)?, width);
    (!rows.is_empty()).then_some(LivePicture {
        head: SettlingHead::Thinking { secs },
        body: SettlingBody::Tail {
            rows,
            dress: SettlingDress::Thought,
            hidden: 0,
            clock: None,
        },
    })
}

/// The picture the running command leaves when it ends with outcome `ok`:
/// its header with the bullet in the outcome's colour, the tail of output
/// its cell is showing, and its clock stopped — `+12 lines (4s)`, no wait
/// left to count and no key left to hint.
///
/// `None` when the front call is not a running command with a cell of its
/// own that has printed something: a `!` command (whose cell ends its own
/// turn), a call that has not started, a tool that is not a command (its
/// settled cell is no shorter than its `⎿ Running…` row), a command folded
/// into the collapsed read/search group (`docs/tool-groups.md`), and a
/// command that has printed nothing — `⎿ Running…` settles into
/// `⎿ (no output)`, a row for a row.
///
/// Take it **before** the call is resolved (`App::end_tool`), which replaces
/// the streamed output the window is read from.
#[must_use]
pub fn command_picture(app: &App, ok: bool, width: u16) -> Option<LivePicture> {
    let paths = app.path_display();
    let (tool, live, elapsed) = match app.viewed_agent() {
        Some(run) => (
            run.tool_queue.front()?,
            run.live_tail(),
            run.command_elapsed,
        ),
        None => {
            let front = app.tool_queue().front()?;
            // The collapsed group's reads are one cell, and it stays open.
            let grouped = !app.settings().verbose && crate::app::group_kind(front, paths).is_some();
            if grouped {
                return None;
            }
            (front, app.live_tail(), app.command_elapsed())
        }
    };
    if tool.status != ToolStatus::Running || tool.shell || !is_command_tool(tool) {
        return None;
    }
    let tail = command_tail(tool, live, width);
    if tail.window.is_empty() {
        return None;
    }
    // The header as the running cell drew it — same text, same wrap — with
    // the bullet at rest in the outcome's colour.
    let mut head = tool_header_lines(tool, width, /*collapsed=*/ true, None, paths);
    let outcome = if ok {
        ToolStatus::Ok
    } else {
        ToolStatus::Failed
    };
    if let Some(bullet) = head.first_mut().and_then(|line| line.spans.first_mut()) {
        *bullet = bullet_span(tool_status_color(outcome), None);
    }
    let secs = elapsed.unwrap_or_default().as_secs();
    Some(LivePicture {
        head: SettlingHead::Lines(head),
        body: SettlingBody::Tail {
            rows: tail.window,
            dress: SettlingDress::Output,
            hidden: tail.hidden,
            clock: Some(format!("({})", format_elapsed(secs))),
        },
    })
}

/// The picture the open read/search group leaves when it closes: its live
/// cell, unchanged — the `● Reading 2 files…` header, whose bullet goes on
/// blinking, over the `⎿` rows naming the value it was showing. `None` when
/// no group is open (or its cell has no row under the header).
///
/// Take it **before** the group is closed (`App::close_tool_group`).
#[must_use]
pub fn group_picture(app: &App, width: u16) -> Option<LivePicture> {
    if app.viewed_agent().is_some() {
        return None;
    }
    // At rest: the renderer puts the blink back per frame.
    let mut lines = super::tool_group::live_group_lines(app, width, None, true);
    (lines.len() > 1).then(|| {
        let body = lines.split_off(1);
        LivePicture {
            head: SettlingHead::Running(lines),
            body: SettlingBody::Rows(body),
        }
    })
}

/// The settling cell for a live cell that just ended: `picture` over the
/// lines it commits (`committed`, without their spacer).
///
/// A cell whose committed form is **no shorter** than the picture — no
/// picture at all, or a settled cell that grew — has nothing to give up, and
/// is what it commits: it only ever waits behind an older cell, and a tail
/// it has already outgrown would be the wrong thing to show meanwhile.
#[must_use]
pub fn settling_cell(picture: Option<LivePicture>, committed: &[Line<'static>]) -> Settling {
    let Some(picture) = picture else {
        return Settling::settled(committed.to_vec());
    };
    let cell = Settling::new(picture.head, picture.body, committed.to_vec());
    if cell.is_settled() {
        Settling::settled(committed.to_vec())
    } else {
        cell
    }
}

/// A settling cell at the rows it keeps, at the frame `pulse`.
///
/// While it holds rows of its own: its head — a command's as it finished, a
/// thought's and a read group's as the live cell drew it ([`SettlingHead`]),
/// still blinking while the cell is **whole** and at rest once it has given
/// a row up, since by then something has landed in its place — then as much
/// of its body as those rows leave: a tail's **newest** rows, the row on top
/// wearing the `⎿` corner, over a footer that counts what is hidden;
/// laid-out rows from the top.
///
/// Down to its committed height it is **what it commits**, line for line.
/// That is the only place a thought's `Thought for …` line or a group's
/// `Read 2 files` line is ever drawn in the strip — alone, never over the
/// content it stands for.
#[must_use]
pub fn settling_lines(cell: &Settling, pulse: Duration) -> Vec<Line<'static>> {
    if cell.is_settled() {
        return cell.committed().to_vec();
    }
    // Whole, a kept-as-it-was head still moves with the frame clock; once
    // the cell has given a row up it stands at rest.
    let blink = cell.is_whole().then_some(pulse);
    let mut lines = match cell.head() {
        SettlingHead::Lines(lines) => lines.clone(),
        SettlingHead::Running(lines) => {
            let mut lines = lines.clone();
            if let Some(bullet) = lines.first_mut().and_then(|line| line.spans.first_mut()) {
                *bullet = bullet_span(tool_running_color(), blink);
            }
            lines
        }
        SettlingHead::Thinking { .. } => vec![match blink {
            Some(pulse) => live_thinking_header(pulse),
            None => resting_thinking_header(),
        }],
    };
    let shown = cell.shown_body_rows();
    match cell.body() {
        SettlingBody::None => {}
        SettlingBody::Tail {
            rows,
            dress,
            hidden,
            clock,
        } => {
            let skipped = rows.len() - shown;
            for (index, row) in rows[skipped..].iter().enumerate() {
                lines.push(match dress {
                    SettlingDress::Output => output_row(index, row.clone()),
                    SettlingDress::Thought => body_row(index, row.clone()),
                });
            }
            if let Some(clock) = clock {
                lines.push(result_row(
                    shown,
                    format!("{}{clock}", hidden_lead(hidden + skipped)),
                ));
            }
        }
        SettlingBody::Rows(rows) => lines.extend(rows[..shown].iter().cloned()),
    }
    lines.truncate(cell.rows());
    lines
}

/// `status` as the strip draws it: with the `Thinking for Ns` clause still on
/// it while a thought's block stands in the strip
/// ([`App::lingering_thought`]). The block and the clause are two halves of
/// one picture — a phase that ended changes neither until its rows are taken
/// — and a phase open right now speaks for itself.
pub(super) fn lingering_status<'a>(app: &App, status: &'a TurnStatus) -> Cow<'a, TurnStatus> {
    match app.lingering_thought() {
        Some(thinking) if status.thinking.is_none() => {
            let mut status = status.clone();
            status.thinking = Some(thinking);
            Cow::Owned(status)
        }
        _ => Cow::Borrowed(status),
    }
}

/// The settling cells as the strip draws them: oldest on top, one blank row
/// between cells — where each will sit in scrollback.
pub(super) fn settling_preview_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for cell in app.settling() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.extend(settling_lines(cell, app.pulse()));
    }
    lines
}

/// The rows [`settling_preview_lines`] takes, without building them.
fn settling_rows(app: &App) -> usize {
    let cells = app.settling();
    cells.iter().map(Settling::rows).sum::<usize>() + cells.len().saturating_sub(1)
}

/// The preview slot's rows with the settling cells standing above `live`
/// rows of live content: the cells, a blank row, the live content — or
/// whichever of the two there is.
pub(super) fn with_settling_rows(app: &App, live: u16) -> u16 {
    if app.settling().is_empty() {
        return live;
    }
    let cells = u16::try_from(settling_rows(app)).unwrap_or(u16::MAX);
    if live == 0 {
        cells
    } else {
        cells.saturating_add(1).saturating_add(live)
    }
}

/// The rows of the preview slot left for **live** content under the
/// settling cells, of `rows` in all: what is left once the cells and the
/// blank row under them are paid.
pub(super) fn live_rows_under_settling(app: &App, rows: u16) -> u16 {
    if app.settling().is_empty() {
        return rows;
    }
    let cells = u16::try_from(settling_rows(app)).unwrap_or(u16::MAX);
    rows.saturating_sub(cells.saturating_add(1))
}

/// What the boundary knows about the frame it is about to paint — the
/// viewport's bookkeeping, handed to the plan the way `ui::repin` and
/// `ui::modal_needs_rebuild` are handed theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleGeometry {
    /// The screen row just below the region as it was last **painted**.
    pub painted_bottom: u16,
    /// The region's top row.
    pub view_top: u16,
    /// Rows already queued for scrollback — the next flush writes them above
    /// the region, pushing it down.
    pub pending_rows: usize,
    /// The region's height for this frame, with the settling cells at the
    /// rows they keep now.
    pub height: u16,
    /// Whether the conversation on screen may hold cells at all
    /// ([`settling_holds`], and the boundary's own conditions).
    pub holds: bool,
}

/// How many rows each settling cell keeps this frame: as few as hold the
/// region's bottom where it was last painted.
///
/// Were every cell to commit now, each row a cell gives up would be a row
/// the region is shorter by (its committed lines and their spacer take the
/// place of its head and the blank under it). If that would end the region
/// above where it was painted, the difference is kept — by the **newest**
/// cells first, so the oldest are the first down to their committed height:
/// scrollback takes cells in order. A frame in which something else grew by
/// as much (the next cell landing, a reply's first row, a commit) keeps
/// nothing, and every cell goes.
///
/// A cell never keeps more than it has: where the region shrank for a
/// reason no cell covers, the box moves by the rest, as it always did.
#[must_use]
pub fn settle_plan(geometry: &SettleGeometry, cells: &[Settling]) -> Vec<usize> {
    let mut keep: Vec<usize> = cells.iter().map(Settling::settled_rows).collect();
    if !geometry.holds {
        return keep;
    }
    let extra = |cell: &Settling| cell.rows().saturating_sub(cell.settled_rows());
    let extras: usize = cells.iter().map(extra).sum();
    let planned =
        (usize::from(geometry.view_top) + geometry.pending_rows + usize::from(geometry.height))
            .saturating_sub(extras);
    let mut need = usize::from(geometry.painted_bottom).saturating_sub(planned);
    for (rows, cell) in keep.iter_mut().zip(cells).rev() {
        let kept = extra(cell).min(need);
        *rows += kept;
        need -= kept;
    }
    keep
}

/// Before a frame: lower each settling cell to the rows [`settle_plan`]
/// gives it, let go of the oldest cells that are down to their committed
/// height, and report how many went — the boundary flushes that many held
/// batches, so the cells' committed lines and whatever took their rows land
/// in the same frame.
///
/// Only a **prefix** goes: a cell with nothing left to give up still waits
/// behind an older one that holds rows.
pub fn settle_frame(app: &mut App, geometry: &SettleGeometry) -> usize {
    let keep = settle_plan(geometry, app.settling());
    for (index, rows) in keep.into_iter().enumerate() {
        app.keep_settling(index, rows);
    }
    let released = app
        .settling()
        .iter()
        .take_while(|cell| cell.is_settled())
        .count();
    app.release_settling(released);
    released
}

/// Whether the conversation on screen may hold settling cells right now: it
/// is the inline conversation view, a turn is in flight in it, no modal
/// prompt has replaced the region, and the terminal has room for the strip.
///
/// Outside a turn nothing follows to take a cell's rows, a prompt's region
/// is not the strip's, and on a terminal too short for the strip its
/// overflow flows into scrollback instead (`docs/strip-flow.md`) — a held
/// cell there would be rows the region does not have.
#[must_use]
pub fn settling_holds(app: &App, width: u16, term_height: u16) -> bool {
    app.view == View::Conversation
        && !app.modal_open()
        && strip_has_status(app)
        && preview_rows(app, width) <= preview_budget(app, width, term_height)
}

/// Whether a just-ended `cell` settles in place rather than committing at
/// once: the conversation may hold cells ([`settling_holds`]) and either the
/// cell has rows to give up, or an older cell is still settling ahead of it
/// — scrollback takes them in order.
#[must_use]
pub fn settles_in_place(app: &App, cell: &Settling, width: u16, term_height: u16) -> bool {
    settling_holds(app, width, term_height) && (!cell.is_settled() || !app.settling().is_empty())
}
