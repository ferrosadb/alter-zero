//! Settling cells: a live cell that has ended, kept on the rows it stood on
//! until whatever comes next takes them. See `docs/settling.md`.
//!
//! The live region is content-anchored (CLAUDE.md invariant 3): a frame that
//! sheds more rows from the strip than it commits to scrollback pulls the box
//! **up** by the difference, and the next content pushes it back down — the
//! flicker between a thought and its tool call, and between one command and
//! the next. A live cell is taller than the line it settles into by design
//! (a thinking block's tail, a running command's tail and clock), so the only
//! way to keep the box still without padding the gap with blank rows is to
//! let the finished cell **keep its rows** until something needs them.
//!
//! This module is the pure half: what a settling cell *is*. The pictures are
//! built by `ui` (it knows the width and the renderers), the decision of how
//! many rows each keeps is `ui::settle_plan`, and the boundary holds the
//! committed lines back until the cell is down to them (`tui::settle`).

use std::time::Duration;

use ratatui::text::Line;

use super::*;

/// How a settling cell's body rows are dressed in the `⎿` gutter — the dress
/// the live cell wore, so the rows read the same the frame it stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlingDress {
    /// A command's output: a dim corner over the reply's own colour.
    Output,
    /// A thought: dim and italic.
    Thought,
}

/// What a settling cell opens on while it still holds rows of its own.
///
/// A cell that has ended keeps the header its live cell wore. A finished
/// **command** shows that it finished — its bullet at rest in the outcome's
/// colour — because that is news. A **thought** and a **read/search group**
/// change nothing at all: the line each commits says *that is over*, and
/// drawn over what it stands for it read as a flash of stale content, so it
/// is only ever drawn alone, once the cell is down to it
/// ([`Settling::committed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlingHead {
    /// Rows drawn as they are — a finished command's header.
    Lines(Vec<Line<'static>>),
    /// A live header, stored at rest: while the cell is whole the bullet its
    /// first row opens on goes on blinking the way a running cell's does. A
    /// read/search group's `● Reading 2 files…`.
    Running(Vec<Line<'static>>),
    /// A thinking phase's live `● Thinking…` header — still blinking and
    /// shimmering while the cell is whole. `secs` is how long the phase ran:
    /// the status line goes on saying `Thinking for {secs}s` beside it, so
    /// nothing on screen changes when the phase ends — only when the next
    /// cell lands.
    Thinking {
        /// The phase's length, in whole seconds.
        secs: u64,
    },
}

impl SettlingHead {
    /// The rows the head takes.
    fn rows(&self) -> usize {
        match self {
            Self::Lines(lines) | Self::Running(lines) => lines.len(),
            Self::Thinking { .. } => 1,
        }
    }
}

/// What a settling cell shows under its head — the rows it gives up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlingBody {
    /// Nothing: the cell stands on its committed rows already. It waits only
    /// because an older cell ahead of it still holds rows of its own, and
    /// scrollback must take them in order.
    None,
    /// Rows that tail-follow — a thought's last rows, a command's last output
    /// — given up **oldest first**, the way the live window dropped them. The
    /// rows are plain text so the row left on top can take the `⎿` corner.
    Tail {
        /// The rows the live cell showed, oldest first.
        rows: Vec<String>,
        /// How they are dressed.
        dress: SettlingDress,
        /// Rows the live window already hid when the cell stopped — what its
        /// `+N lines` footer counted.
        hidden: usize,
        /// The footer's clock clause — `(2s)` — when the cell closes on a
        /// footer row: a command's. `None` for a thought, which has none.
        clock: Option<String>,
    },
    /// Rows already laid out — a read/search group's `⎿` hint — given up from
    /// the **bottom**, so the corner row is the last to go.
    Rows(Vec<Line<'static>>),
}

/// A live cell that has ended and is still on screen: its head as the live
/// cell left it, its body the rows the live cell stood on, and the lines it
/// commits.
///
/// `rows` is how many it keeps this frame — its full height the frame it
/// stops, never fewer than [`settled_rows`](Settling::settled_rows), and only
/// ever lowered (`ui::settle_plan`): a row given up is a row something else
/// is standing on. Down to its committed height it **is** what it commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settling {
    head: SettlingHead,
    body: SettlingBody,
    committed: Vec<Line<'static>>,
    settled_rows: usize,
    rows: usize,
}

impl Settling {
    /// A settling cell over `head` and `body` that commits `committed`. It
    /// opens at its full height — the live cell's.
    #[must_use]
    pub fn new(head: SettlingHead, body: SettlingBody, committed: Vec<Line<'static>>) -> Self {
        let mut cell = Self {
            head,
            body,
            settled_rows: committed.len(),
            committed,
            rows: 0,
        };
        cell.rows = cell.full_rows();
        // A committed form taller than the picture has nothing to give up.
        cell.settled_rows = cell.settled_rows.min(cell.rows);
        cell
    }

    /// A cell that stands on its committed rows already: `lines` are what it
    /// commits, drawn as they are until the cells ahead of it have gone.
    #[must_use]
    pub fn settled(lines: Vec<Line<'static>>) -> Self {
        let rows = lines.len();
        Self {
            head: SettlingHead::Lines(Vec::new()),
            body: SettlingBody::None,
            committed: lines,
            settled_rows: rows,
            rows,
        }
    }

    /// What it opens on while it holds rows of its own.
    #[must_use]
    pub const fn head(&self) -> &SettlingHead {
        &self.head
    }

    /// What it shows under the head.
    #[must_use]
    pub const fn body(&self) -> &SettlingBody {
        &self.body
    }

    /// The lines it commits — and what it is drawn as once it is down to
    /// their height.
    #[must_use]
    pub fn committed(&self) -> &[Line<'static>] {
        &self.committed
    }

    /// Whether the body closes on a footer row (a command's clock).
    const fn foot_rows(&self) -> usize {
        match &self.body {
            SettlingBody::Tail { clock: Some(_), .. } => 1,
            _ => 0,
        }
    }

    /// How many body rows there are to show at most.
    fn body_len(&self) -> usize {
        match &self.body {
            SettlingBody::None => 0,
            SettlingBody::Tail { rows, .. } => rows.len(),
            SettlingBody::Rows(rows) => rows.len(),
        }
    }

    /// The rows the cell stood on when it stopped: head, body and footer.
    #[must_use]
    pub fn full_rows(&self) -> usize {
        self.head.rows() + self.body_len() + self.foot_rows()
    }

    /// The rows its committed form takes — what it shrinks toward, and the
    /// height at which it leaves the strip for scrollback.
    #[must_use]
    pub const fn settled_rows(&self) -> usize {
        self.settled_rows
    }

    /// The rows it keeps this frame.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// How many body rows it shows at its current height.
    #[must_use]
    pub fn shown_body_rows(&self) -> usize {
        self.rows
            .saturating_sub(self.head.rows() + self.foot_rows())
            .min(self.body_len())
    }

    /// Whether it still stands on every row the live cell did: it has given
    /// nothing up, so nothing has landed in its place and — a thought, a
    /// read group — it is drawn exactly as the live cell was. Once a row has
    /// gone it stands at rest.
    #[must_use]
    pub fn is_whole(&self) -> bool {
        self.rows >= self.full_rows()
    }

    /// Whether it has nothing left to give up — it is down to its committed
    /// height, so it commits as soon as scrollback order lets it.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.rows <= self.settled_rows
    }

    /// Lower the rows it keeps to `rows` — clamped between its committed
    /// height and what it keeps now, so a cell never grows back.
    pub fn keep(&mut self, rows: usize) {
        self.rows = rows.clamp(self.settled_rows, self.rows.max(self.settled_rows));
    }
}

impl App {
    /// The settling cells on screen, oldest first — what the strip draws
    /// above everything live (`docs/settling.md`).
    #[must_use]
    pub fn settling(&self) -> &[Settling] {
        &self.settling
    }

    /// Put a just-ended cell in the strip, behind the ones already settling.
    /// The boundary holds its committed lines back in step
    /// (`InlineViewport::hold`).
    pub fn push_settling(&mut self, cell: Settling) {
        self.settling.push(cell);
    }

    /// Lower the rows the `index`-th settling cell keeps
    /// ([`Settling::keep`]). Out of range is a no-op.
    pub fn keep_settling(&mut self, index: usize, rows: usize) {
        if let Some(cell) = self.settling.get_mut(index) {
            cell.keep(rows);
        }
    }

    /// Let the oldest `count` settling cells go — their committed lines are
    /// on their way to scrollback.
    pub fn release_settling(&mut self, count: usize) {
        let count = count.min(self.settling.len());
        self.settling.drain(..count);
    }

    /// Let every settling cell go: the conversation on screen moved on
    /// without them — a rebuild repaints them from history, or their lines
    /// were committed ahead of something else.
    pub fn clear_settling(&mut self) {
        self.settling.clear();
    }

    /// How long the thinking phase ran whose block still stands **whole** in
    /// the strip ([`SettlingHead::Thinking`], [`Settling::is_whole`]) —
    /// `None` when no thought's block does. The status line goes on showing
    /// `Thinking for Ns` for it, the block's other half, until something
    /// lands and takes a row: from then the thought is visibly over.
    #[must_use]
    pub fn lingering_thought(&self) -> Option<Duration> {
        self.settling
            .iter()
            .rev()
            .find_map(|cell| match cell.head() {
                SettlingHead::Thinking { secs } if cell.is_whole() && !cell.is_settled() => {
                    Some(Duration::from_secs(*secs))
                }
                _ => None,
            })
    }
}
