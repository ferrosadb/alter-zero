//! The headless run (`docs/cli.md` `-p/--print`, `--output-format
//! stream-json`): the **same** agent loop the TUI runs — the same `App`, the
//! same backend seam, the same context derivation — with no viewport, no raw
//! mode and no alternate screen.
//!
//! What changes is only where the events go. The TUI folds a backend event
//! into `App` *and* commits rendered lines to scrollback; this folds the same
//! event into the same `App` and writes a JSON record instead
//! ([`crate::output`]). So a turn's history — and therefore the next turn's
//! context — is built exactly as it is interactively, while stdout stays pure
//! NDJSON.
//!
//! Two consequences worth stating plainly, because they are what make the mode
//! usable unattended:
//!
//! - **Many turns per process.** `--print` takes the `[PROMPT]` as its first
//!   turn and then one turn per line of stdin until EOF — a voice call opens
//!   the process once and speaks into it. A blank line is not a turn.
//! - **No inline modal can be answered.** A backend failure is a record and
//!   the session carries on; an `AskUserQuestion` is declined on the caller's
//!   behalf (the model is told the user is not answering), because nobody is
//!   at a keyboard to pick an option. Permissions are not raised at all — the
//!   assembly attaches no gate (`ALTER_ZERO_PERMISSIONS=0`'s behaviour), so a
//!   turn cannot park on a prompt that will never come.

use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::app::App;
use crate::ask::{AskDecision, AskGate};
use crate::context;
use crate::output::{JsonlWriter, TurnRecords, init_record, user_record};
use crate::stream::{CancelToken, ReplySource, StreamEvent};

/// What a headless run reports about itself in its first record.
pub struct Options<'a> {
    /// The model answering (the backend's own name).
    pub model: &'a str,
    /// The run's session id — the handle a resuming run passes back.
    pub session_id: &'a str,
    /// The working directory the turn runs in.
    pub cwd: &'a Path,
    /// The session's ask gate, where a question is declined on the absent
    /// user's behalf. `None` leaves the blocked tool thread to the backend's
    /// own cancel — attach one whenever the backend can offer the tool.
    pub ask: Option<&'a AskGate>,
}

/// Run the loop: `prompt` as the first turn (if any), then one turn per line
/// read from `turns` until EOF. Every record goes to `out`, one line each.
///
/// `ask`, when the caller attached one, is where an `AskUserQuestion` is
/// declined on the absent user's behalf — without it the tool thread would
/// park forever.
///
/// # Errors
/// Only the writer's failures: a backend error is a record, not an `Err`, so
/// one failed turn never takes the process down mid-conversation.
pub async fn run<W, R>(
    backend: &dyn ReplySource,
    options: Options<'_>,
    prompt: Option<String>,
    turns: Option<R>,
    out: W,
) -> io::Result<()>
where
    W: Write,
    R: BufRead,
{
    let mut writer = JsonlWriter::new(out);
    let mut app = App::new();
    writer.write(&init_record(
        options.session_id,
        options.model,
        options.cwd,
    ))?;
    if let Some(prompt) = prompt {
        run_turn(&mut app, backend, &options, &prompt, &mut writer).await?;
    }
    if let Some(mut turns) = turns {
        let mut line = String::new();
        loop {
            line.clear();
            if turns.read_line(&mut line)? == 0 {
                break;
            }
            let text = line.trim_end_matches(['\n', '\r']);
            if text.trim().is_empty() {
                continue;
            }
            run_turn(&mut app, backend, &options, text, &mut writer).await?;
        }
    }
    Ok(())
}

/// One turn on the shared [`App`]: record the user message, derive the context
/// the way the TUI does, spawn the backend, and fold every event into `App`
/// while projecting it into records.
async fn run_turn<W: Write>(
    app: &mut App,
    backend: &dyn ReplySource,
    options: &Options<'_>,
    text: &str,
    writer: &mut JsonlWriter<W>,
) -> io::Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    // The user turn first, then the stream opens — the TUI's `start_turn`
    // order, so the derived context carries the message just said.
    app.record_user_message(text);
    app.begin_stream();
    writer.write(&user_record(text))?;
    let context = context::context_messages_full(
        app.user_instructions.as_deref(),
        app.listings.as_deref(),
        &app.history,
    );
    let mut records = TurnRecords::new();
    let handle = backend.spawn(text.to_string(), Vec::new(), context, tx, CancelToken::new());
    // The backend owns the only other sender; when its thread ends the channel
    // closes and the loop below finishes.
    while let Some(event) = rx.recv().await {
        fold(app, &event, options.ask);
        for record in records.on_event(&event) {
            writer.write(&record)?;
        }
    }
    for record in records.finish(options.session_id) {
        writer.write(&record)?;
    }
    // The backend thread has already ended (the channel closed); its handle is
    // simply dropped, never joined — a failed write must not wait on it.
    drop(handle);
    Ok(())
}

/// Fold one backend event into the pure [`App`] — the headless twin of
/// `Session::on_stream_event`, minus every commit. The App-side calls are the
/// loop's own methods, so a turn's recorded history (and the next turn's
/// context) is what an interactive session would have.
fn fold(app: &mut App, event: &StreamEvent, ask: Option<&AskGate>) {
    match event {
        StreamEvent::Chunk(text) => app.push_chunk(text),
        StreamEvent::RoundCalls(calls) => app.open_round(calls.clone()),
        StreamEvent::ToolBatch(items) => app.start_tool_batch(items),
        StreamEvent::ToolStart {
            name,
            args,
            arguments,
            ..
        } => app.start_tool(name, args, arguments.as_deref()),
        StreamEvent::ToolEnd {
            output,
            ok,
            truncated,
        } => {
            if *truncated {
                app.set_tool_truncated();
            }
            app.end_tool(output, *ok);
        }
        StreamEvent::ToolRejected {
            display,
            result,
            truncated,
        } => {
            if *truncated {
                app.set_tool_truncated();
            }
            app.reject_tool(display, result);
        }
        StreamEvent::ToolAnswered {
            display,
            result,
            truncated,
        } => {
            if *truncated {
                app.set_tool_truncated();
            }
            app.answer_tool(display, result);
        }
        StreamEvent::ToolBackgrounded { output, .. } => {
            app.background_tool(output);
        }
        StreamEvent::TaskCall {
            name,
            args,
            arguments,
            output,
            ok,
            tasks,
        } => app.record_task_call(name, args, arguments, output, *ok, tasks.clone()),
        StreamEvent::ToolNote(note) => app.set_tool_note(note),
        StreamEvent::ThinkingStart => app.begin_reasoning(),
        StreamEvent::ThinkingChunk(chunk) => app.push_thinking(chunk),
        // The boundary owns the phase clock; a headless turn records the phase
        // without one (its `Thought for 0s` never reaches an answer a caller
        // hears, and the reasoning text is never emitted at all).
        StreamEvent::ThinkingEnd => {
            app.finish_reasoning(0);
        }
        StreamEvent::Usage(usage) => app.apply_usage(usage),
        StreamEvent::Retrying { attempt, max } => {
            let _ = app.withdraw_attempt();
            app.set_retry(*attempt, *max);
        }
        StreamEvent::PromptBlocked { reason } => {
            app.block_prompt(reason);
        }
        StreamEvent::Error(message) => {
            app.fail_stream(message);
        }
        StreamEvent::StreamDone => {
            app.finish_stream();
        }
        // The model asked the user something and no user is here to answer:
        // decline so the blocked tool thread wakes with the stop-and-wait
        // result instead of parking forever. Never raised in `App` — a modal
        // that no one could answer would leave its state stuck.
        StreamEvent::AskUser(request) => {
            if let Some(gate) = ask {
                gate.resolve(&request.id, AskDecision::Declined);
            }
        }
        // Live-only, subagent and steering events: they exist for the TUI's
        // roster, queues and status line. A headless turn has none of those,
        // and the record projection ignores them too.
        StreamEvent::AgentBatch { .. }
        | StreamEvent::AgentGroupDone { .. }
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::ToolOutput(_)
        | StreamEvent::ToolScreen { .. }
        | StreamEvent::ToolTitle(_)
        | StreamEvent::HookNote { .. }
        | StreamEvent::Steered { .. }
        | StreamEvent::NoticeDelivered { .. }
        | StreamEvent::Offline { .. }
        | StreamEvent::Permission(_) => {}
    }
}
