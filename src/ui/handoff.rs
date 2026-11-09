//! The strip's **hand-off hold** — the box holds still while one live cell
//! gives way to the next. See `docs/handoff-hold.md`.
//!
//! The live region is content-anchored (CLAUDE.md invariant 3): a frame that
//! commits `c` rows to scrollback pushes it down by `c`, and a frame whose
//! region is `h` rows shorter pulls its bottom up by `h`. So the box rises
//! exactly when the strip sheds more rows than went to scrollback in the same
//! frame — and every hand-off between live cells can: a thought settles into
//! its one-line `Thought for …` cell before the round's call has a cell of
//! its own, a running command's four-row tail settles into a shorter peek, a
//! read group's two-row cell into its one line. The box rose by the
//! difference and fell back when the next cell landed — a frame or a few
//! later: the reported flicker.
//!
//! The strip now keeps the rows it stood on, less what committed, as blank
//! rows over its status line — for as long as the next cell is plausibly
//! about to land, and not a moment longer.

use std::time::Instant;

use super::layout::{fit_preview_rows, non_preview_rows};
use super::theme::{GAP_ROWS, HANDOFF_HOLD, HANDOFF_HOLD_MAX, HANDOFF_HOLD_RUN};
use super::*;

/// The boundary-owned tracker behind the hand-off hold — `StreamRender`'s
/// sibling: it remembers what the last painted frame's preview slot stood
/// on, so the next frame can be held to it (`docs/handoff-hold.md`).
///
/// Each conversation-view frame the boundary zeroes the injected hold, asks
/// [`rows`](Self::rows), injects the answer ([`App::set_handoff_hold`]),
/// paints, and [`record`](Self::record)s. A purge rebuild — which re-seats
/// the region from scratch — starts it over ([`reset`](Self::reset)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandoffHold {
    /// Rows the preview slot stood on in the last painted frame: its content,
    /// the content's gap, and the rows held over the status line. 0 when that
    /// frame painted no live strip.
    slot: u16,
    /// When the hold in force began — the first frame that had to hold a row.
    /// `None` when nothing is held.
    since: Option<Instant>,
}

impl HandoffHold {
    /// The blank rows to hold over the status line in the frame about to be
    /// painted, given the `committed` rows queued for scrollback since the
    /// last painted frame and the boundary's clock.
    ///
    /// The slot owes what it stood on less what committed; the strip's own
    /// content pays what it can, and the rest is held — but only while the
    /// hold is young: each hand-off on its own clock, for as long as
    /// `hold_limit` allows in the state the strip is in (a call being
    /// written or the reply streaming, a tool running, or nothing on its
    /// way). Never more than the preview's budget leaves (a short terminal
    /// keeps its composer), and nothing outside a live strip — idle, a `!`
    /// shell turn, a modal prompt.
    ///
    /// Decided from the strip's **own** rows: the hold injected for the last
    /// frame is left out of every measure here, so it cannot feed itself.
    pub fn rows(
        &mut self,
        app: &App,
        committed: usize,
        width: u16,
        height: u16,
        now: Instant,
    ) -> u16 {
        if !holds_strip(app) {
            self.since = None;
            return 0;
        }
        // Every row of the region but the preview slot and the hold.
        let other = non_preview_rows(app, width).saturating_sub(applied_handoff_hold(app));
        let content = slot_rows(fit_preview_rows(height, preview_rows(app, width), other));
        // The most the slot may take: what the region has left once
        // everything else is paid.
        let room = height.saturating_sub(other);
        let owed = u16::try_from(usize::from(self.slot).saturating_sub(committed))
            .unwrap_or(u16::MAX)
            .min(room);
        let want = owed.saturating_sub(content);
        if want == 0 {
            self.since = None;
            return 0;
        }
        // Each hand-off gets its own clock: a frame that committed rows and
        // still owes some is a cell giving way to the next — a new hand-off,
        // not the old one running on.
        if committed > 0 {
            self.since = Some(now);
        }
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= hold_limit(app) {
            return 0;
        }
        want
    }

    /// Remember what the frame just painted stood on: its preview slot and
    /// the rows held over the status line — the next frame's starting point.
    /// Nothing outside a live strip: there is nothing on screen to hold.
    pub fn record(&mut self, app: &App, width: u16, height: u16) {
        if holds_strip(app) {
            self.slot = slot_rows(fitted_preview_rows(app, width, height))
                .saturating_add(applied_handoff_hold(app));
        } else {
            *self = Self::default();
        }
    }

    /// Forget what the last frame stood on — a purge rebuild re-seats the
    /// region, so there is no bottom left to hold.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The rows a preview slot of `preview` content rows stands on: the rows and
/// their gap, or nothing at all (`strip_rows`' own rule).
const fn slot_rows(preview: u16) -> u16 {
    if preview > 0 {
        preview.saturating_add(GAP_ROWS)
    } else {
        0
    }
}

/// Whether the region shows a **live strip** a hold may keep still: a status
/// line on screen — the viewed agent's run inside its session view, the main
/// turn otherwise, never a `!` shell turn (which has none, and whose end is
/// the turn's end) — and no modal prompt, which replaces the whole region and
/// purge-rebuilds on its close.
fn holds_strip(app: &App) -> bool {
    !app.modal_open() && strip_has_status(app)
}

/// Whether the conversation on screen is still **writing a tool call** this
/// round — the call's cell is on its way, so the hold may wait for it
/// ([`HANDOFF_HOLD_MAX`]).
fn generating(app: &App) -> bool {
    app.viewed_agent().map_or_else(
        || app.tool_drafts().is_generating(),
        |run| run.drafts().is_generating(),
    )
}

/// The held rows that apply to `app` now — the boundary-injected
/// [`App::handoff_hold`] while a live strip is up, else 0, so a value
/// injected a frame ago can never pad an idle composer. Counted by
/// [`hang_rows`] and painted over the status line by `strip_lines`.
pub(super) fn applied_handoff_hold(app: &App) -> u16 {
    if holds_strip(app) {
        app.handoff_hold()
    } else {
        0
    }
}

/// Whether a thought whose phase ended `ended_for` ago has lingered long
/// enough and settles now, at the draw tick (`docs/handoff-hold.md` *The
/// thought lingers*).
///
/// The backend ends a phase at the round's first tool-call piece, while the
/// call has a cell only once its batch is announced; so the ended phase keeps
/// its live block up, unchanged, and the first event that draws or records
/// something settles it in the frame its own content lands
/// (`StreamEvent::settles_ended_thought`). This is the other way it settles:
/// the hold's own clock — up to `HANDOFF_HOLD_MAX` while the round is still
/// writing a call (its cell is on its way), `HANDOFF_HOLD` otherwise — past
/// which a block left up would claim a thought that is over.
///
/// Only the main conversation's thought lingers (a subagent's session view
/// settles its own at once), so the call it waits on is the main turn's —
/// whichever conversation is on screen.
#[must_use]
pub fn thought_linger_due(app: &App, ended_for: std::time::Duration) -> bool {
    let limit = if app.tool_drafts().is_generating() {
        HANDOFF_HOLD_MAX
    } else {
        HANDOFF_HOLD
    };
    ended_for >= limit
}

/// How long a hold may last in `app`'s state — a bet that the next commit or
/// cell lands before it, so the box never moves twice
/// (`docs/handoff-hold.md` *How long*):
///
/// - `HANDOFF_HOLD_MAX` while something is **certainly on its way**: a call
///   being written (its cell lands with the round's announcement), or the
///   reply's text streaming (its next line commits and pays the hold off);
/// - `HANDOFF_HOLD_RUN` while a **tool is running**: a quick one settles
///   inside it, and its commit pays the hold off in the same frame — but a
///   command can run for minutes, and blank rows held under it for seconds
///   are the rejected floor's;
/// - `HANDOFF_HOLD` otherwise — an empty strip, a read group idling between
///   rounds: the next cell is the next request's.
pub(super) fn hold_limit(app: &App) -> std::time::Duration {
    if generating(app) || streaming_reply(app) {
        HANDOFF_HOLD_MAX
    } else if running(app) {
        HANDOFF_HOLD_RUN
    } else {
        HANDOFF_HOLD
    }
}

/// Whether the conversation on screen is streaming reply text — anything but
/// blank, which commits nothing.
fn streaming_reply(app: &App) -> bool {
    let text = app
        .viewed_agent()
        .map_or_else(|| app.streaming_text(), |run| run.streaming.as_deref());
    text.is_some_and(|text| !text.trim().is_empty())
}

/// Whether a tool is running in the conversation on screen — the front of
/// its queue, or the main turn's live agent group.
fn running(app: &App) -> bool {
    match app.viewed_agent() {
        Some(run) => run
            .tool_queue
            .front()
            .is_some_and(|call| call.status == ToolStatus::Running),
        None => {
            app.agent_group().is_some()
                || app
                    .tool_queue()
                    .front()
                    .is_some_and(|call| call.status == ToolStatus::Running)
        }
    }
}
