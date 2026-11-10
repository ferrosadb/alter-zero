//! Settling cells at the boundary: holding a finished cell's committed lines
//! back until the frame that lets it go. See `docs/settling.md`.
//!
//! A live cell is taller than the lines it commits — a thinking block's
//! tail, a running command's tail and clock — and the region is
//! content-anchored (invariant 3), so committing the short form the moment
//! the cell ended pulled the box **up** by the difference until the next
//! content pushed it back: the flicker between a thought and its tool call,
//! and between one command and the next. So a cell that ends **settles in
//! place** instead: `App` keeps its picture in the strip
//! ([`alter_zero::app::Settling`]) on the rows the live cell stood on, and
//! its committed lines wait in the viewport (`InlineViewport::hold`) until
//! the plan ([`ui::settle_frame`]) says the rows it gives up have been taken.
//!
//! Everything that decides is pure and unit-tested in `ui::settling` — what a
//! cell leaves, how it is drawn, how many rows it keeps. This module is the
//! two places the boundary acts on it:
//!
//! - [`Session::commit_cell`] — the one commit site for a **cell**, a block
//!   that stands apart under a blank row of its own: hold it, or commit it
//!   at once;
//! - [`Session::sync_settling`] — run before every inline frame: apply the
//!   plan and release the cells that are down to their committed height.
//!
//! Scrollback order needs no bookkeeping here. A cell that lands while an
//! older one is settling waits behind it (so a message delivered at the
//! round boundary straight after a command cannot push that command out
//! with its rows untaken); anything else — a reply's lines — goes through
//! `InlineViewport::insert_before`, which queues the held lines first. And a
//! rebuild from history ([`Session::drop_settling`]) drops the hold, since the
//! rebuilt tail carries those cells.

use ratatui::text::Line;

use alter_zero::ui;

use super::Session;

impl Session<'_> {
    /// Commit a **cell** — a block that stands apart in scrollback under a
    /// blank row of its own: a resolved tool call, a settled thought, a
    /// closed read/search group, and whatever lands between two of them (a
    /// delivered message's bubble, a notice, a hook's cell) — or let it
    /// settle in place. `lines` are what it commits (without their spacer),
    /// and `picture` is what a **live** cell left in the strip, taken before
    /// it was resolved (`ui::command_picture` and its siblings); `None` for
    /// a cell with nothing to hold.
    ///
    /// The cell settles in place when it has rows to give up, or when an
    /// older cell is still settling ahead of it: scrollback takes them in
    /// order, and a cell that pushed the older one out would take its rows
    /// away before anything stood on them (`ui::settles_in_place`).
    /// Otherwise it commits now, after the reseat every post-strip commit
    /// makes (invariant 3).
    ///
    /// The caller has already checked [`Session::commits_allowed`].
    pub(crate) fn commit_cell(
        &mut self,
        lines: Vec<Line<'static>>,
        picture: Option<ui::LivePicture>,
    ) {
        let screen = self.term.screen();
        let cell = ui::settling_cell(picture, &lines);
        let mut batch = lines;
        batch.push(Line::default());
        if self.screen_holds_cells()
            && ui::settles_in_place(&self.app, &cell, screen.width, screen.height)
        {
            self.term.hold(batch);
            self.app.push_settling(cell);
            return;
        }
        let height = self.live_region_height();
        self.term.set_view_height(height);
        self.term.insert_before(batch);
    }

    /// The boundary's own conditions for holding a cell, beside the pure
    /// `ui::settling_holds`: no framed view's page is flowed into scrollback
    /// above the region. A commit under a flow would land between the flowed
    /// page and its painted tail (`docs/view-flow.md`), and the flow's exit
    /// rebuilds from history anyway.
    fn screen_holds_cells(&self) -> bool {
        self.flowed_view.is_none()
    }

    /// Before an inline frame: give the settling cells the rows this frame
    /// can spare them, and release the ones that are down to their committed
    /// height — their lines are queued here, so the frame that draws whatever
    /// took their rows writes them with it (`ui::settle_frame`).
    ///
    /// A commit that was not a cell's has already released every held batch
    /// (`InlineViewport::insert_before`), and a conversation reset drops the
    /// cells without their lines: either way the two sides are out of step,
    /// and everything still held goes — committed content is never lost.
    pub(crate) fn sync_settling(&mut self) {
        if self.app.settling().len() != self.term.held_len() {
            self.term.release_held(usize::MAX);
            self.app.clear_settling();
        }
        if self.app.settling().is_empty() {
            return;
        }
        let screen = self.term.screen();
        let geometry = ui::SettleGeometry {
            painted_bottom: self.term.painted_bottom(),
            view_top: self.term.view_top(),
            pending_rows: self.term.pending_rows(),
            height: self.live_region_height(),
            holds: self.screen_holds_cells()
                && ui::settling_holds(&self.app, screen.width, screen.height),
        };
        let released = ui::settle_frame(&mut self.app, &geometry);
        self.term.release_held(released);
    }

    /// Let the settling cells go **unwritten**: the screen is about to be
    /// rebuilt from history, which carries them — the rebuild
    /// (`InlineViewport::reflow`) drops their held lines with the queue.
    pub(crate) fn drop_settling(&mut self) {
        self.app.clear_settling();
    }
}
