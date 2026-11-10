//! Settling cells: the data a finished live cell keeps on screen, and the
//! list `App` holds of them. See `docs/settling.md`.

use ratatui::text::Line;

use std::time::Duration;

use super::*;
use crate::app::{Settling, SettlingBody, SettlingDress, SettlingHead};

fn tail(rows: &[&str], hidden: usize, clock: Option<&str>) -> SettlingBody {
    SettlingBody::Tail {
        rows: rows.iter().map(ToString::to_string).collect(),
        dress: SettlingDress::Output,
        hidden,
        clock: clock.map(str::to_string),
    }
}

fn rows(count: usize) -> Vec<Line<'static>> {
    (0..count)
        .map(|row| Line::from(format!("row {row}")))
        .collect()
}

/// A finished command's `count`-row header.
fn head(count: usize) -> SettlingHead {
    SettlingHead::Lines(rows(count))
}

/// The `count` lines a cell commits.
fn committed(count: usize) -> Vec<Line<'static>> {
    (0..count)
        .map(|row| Line::from(format!("committed {row}")))
        .collect()
}

/// A thought's rows under its live header.
fn thought(rows: &[&str]) -> SettlingBody {
    SettlingBody::Tail {
        rows: rows.iter().map(ToString::to_string).collect(),
        dress: SettlingDress::Thought,
        hidden: 0,
        clock: None,
    }
}

#[test]
fn a_settling_cell_opens_on_the_rows_the_live_cell_stood_on() {
    // A command's cell: one header row, four rows of output, the clock row —
    // six in all, of which its committed form keeps five.
    let cell = Settling::new(
        head(1),
        tail(&["a", "b", "c", "d"], 9, Some("(2s)")),
        committed(5),
    );
    assert_eq!(cell.full_rows(), 6);
    assert_eq!(cell.rows(), 6, "it starts where the live cell was");
    assert_eq!(cell.settled_rows(), 5);
    assert_eq!(cell.shown_body_rows(), 4);
    assert!(!cell.is_settled());
}

#[test]
fn a_thought_has_no_footer_row() {
    // The live header over three rows of thought: four rows, one committed.
    let cell = Settling::new(
        SettlingHead::Thinking { secs: 2 },
        thought(&["x", "y", "z"]),
        committed(1),
    );
    assert_eq!(cell.full_rows(), 4, "the header is one row");
    assert_eq!(cell.shown_body_rows(), 3);
    assert_eq!(cell.settled_rows(), 1);
}

#[test]
fn a_cell_keeps_the_lines_it_commits() {
    // What reaches scrollback when the cell is let go — and what it is drawn
    // as once it is down to that height.
    let cell = Settling::new(head(1), tail(&["a", "b"], 0, Some("(2s)")), committed(3));
    assert_eq!(cell.committed(), committed(3));
    assert_eq!(Settling::settled(committed(2)).committed(), committed(2));
}

#[test]
fn keeping_fewer_rows_takes_them_from_the_body() {
    let mut cell = Settling::new(
        head(1),
        tail(&["a", "b", "c", "d"], 0, Some("(2s)")),
        committed(3),
    );
    cell.keep(4);
    assert_eq!(cell.rows(), 4);
    assert_eq!(
        cell.shown_body_rows(),
        2,
        "the header and the footer stay; the body gives the rows up"
    );
}

#[test]
fn a_cell_never_keeps_fewer_rows_than_it_commits() {
    let mut cell = Settling::new(head(1), tail(&["a", "b"], 0, Some("(2s)")), committed(3));
    cell.keep(0);
    assert_eq!(cell.rows(), 3, "clamped at the committed height");
    assert!(cell.is_settled());
}

#[test]
fn a_cell_never_grows_back() {
    // A row given up is a row something else is standing on.
    let mut cell = Settling::new(head(1), tail(&["a", "b", "c"], 0, None), committed(1));
    cell.keep(2);
    cell.keep(4);
    assert_eq!(cell.rows(), 2);
}

#[test]
fn a_cell_on_its_committed_rows_has_nothing_to_give_up() {
    let cell = Settling::settled(committed(3));
    assert_eq!(cell.rows(), 3);
    assert_eq!(cell.settled_rows(), 3);
    assert!(cell.is_settled());
    assert_eq!(cell.shown_body_rows(), 0);
}

#[test]
fn a_committed_form_taller_than_the_picture_is_already_settled() {
    // The settled cell grew (a command whose final output outran what it
    // streamed): nothing is held on its account.
    let cell = Settling::new(head(1), tail(&["a"], 0, Some("(1s)")), committed(9));
    assert!(cell.is_settled());
    assert_eq!(cell.settled_rows(), cell.rows());
}

#[test]
fn opaque_rows_count_like_any_body() {
    let body = SettlingBody::Rows(vec![Line::from("  ⎿  about.py")]);
    let cell = Settling::new(SettlingHead::Running(rows(1)), body, committed(1));
    assert_eq!(cell.full_rows(), 2);
    assert_eq!(cell.shown_body_rows(), 1);
}

#[test]
fn the_app_keeps_settling_cells_oldest_first() {
    let mut app = App::new();
    assert!(app.settling().is_empty());
    app.push_settling(Settling::settled(committed(1)));
    app.push_settling(Settling::settled(committed(2)));
    let rows: Vec<usize> = app.settling().iter().map(Settling::rows).collect();
    assert_eq!(rows, [1, 2]);
}

#[test]
fn releasing_takes_the_oldest_cells() {
    let mut app = App::new();
    for rows in 1..=3 {
        app.push_settling(Settling::settled(committed(rows)));
    }
    app.release_settling(2);
    let rows: Vec<usize> = app.settling().iter().map(Settling::rows).collect();
    assert_eq!(rows, [3], "scrollback takes them in order");
    app.release_settling(9);
    assert!(app.settling().is_empty(), "releasing past the end is safe");
}

#[test]
fn keeping_rows_reaches_one_cell() {
    let mut app = App::new();
    app.push_settling(Settling::new(
        head(1),
        tail(&["a", "b", "c"], 0, None),
        committed(1),
    ));
    app.push_settling(Settling::new(
        head(1),
        tail(&["d", "e", "f"], 0, None),
        committed(1),
    ));
    app.keep_settling(0, 2);
    app.keep_settling(7, 1);
    let rows: Vec<usize> = app.settling().iter().map(Settling::rows).collect();
    assert_eq!(rows, [2, 4]);
}

#[test]
fn a_cleared_conversation_holds_no_settling_cell() {
    // `/clear`, a `/resume` load and a rewind each replace the conversation
    // the cells belonged to.
    let mut app = App::new();
    app.push_settling(Settling::settled(committed(1)));
    app.clear_conversation();
    assert!(app.settling().is_empty());

    app.push_settling(Settling::settled(committed(1)));
    app.load_session(Vec::new());
    assert!(app.settling().is_empty());
}

#[test]
fn a_thought_whose_block_still_stands_reports_how_long_it_thought() {
    // The status line goes on saying `Thinking for 7s` beside the block: the
    // phase is over, but nothing on screen says so until its rows are taken.
    let mut app = App::new();
    assert_eq!(app.lingering_thought(), None, "nothing settling");
    app.push_settling(Settling::new(
        head(1),
        tail(&["a", "b"], 0, Some("(2s)")),
        committed(2),
    ));
    assert_eq!(app.lingering_thought(), None, "a command is no thought");
    app.push_settling(Settling::new(
        SettlingHead::Thinking { secs: 7 },
        thought(&["x", "y", "z"]),
        committed(1),
    ));
    assert_eq!(app.lingering_thought(), Some(Duration::from_secs(7)));
    // Once it has given a row up something else has landed: the thought is
    // visibly over, and the clause goes with the block's blink.
    app.keep_settling(1, 3);
    assert_eq!(app.lingering_thought(), None);
}

#[test]
fn a_cell_is_whole_until_it_gives_a_row_up() {
    let mut cell = Settling::new(
        SettlingHead::Thinking { secs: 1 },
        thought(&["x", "y", "z"]),
        committed(1),
    );
    assert!(cell.is_whole());
    cell.keep(3);
    assert!(!cell.is_whole());
    assert!(
        Settling::settled(committed(2)).is_whole(),
        "nothing to give up"
    );
}
