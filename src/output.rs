//! Machine-readable output for the headless mode (`docs/cli.md`): the NDJSON
//! records a `--output-format stream-json` run writes, one per line.
//!
//! The consuming caller is a phone voice agent, so two shapes are a contract
//! rather than a preference: an assistant message
//! (`{"type":"assistant","message":{"role":"assistant","content":[{"type":"text",
//! "text":"…"}]}}`) and a streamed text delta
//! (`{"type":"stream_event","event":{"type":"content_block_delta","delta":
//! {"type":"text_delta","text":"…"}}}`). Both are built here and nowhere else.
//!
//! Two rules follow from *why* the mode exists:
//!
//! - **A tool call is never text.** It is announced as a `tool_use` content
//!   block and resolved by a `tool_result` record, so a caller that reads only
//!   text blocks speaks the answer and nothing else. The arguments and output
//!   never enter an assistant message.
//! - **Nothing renders.** A record is JSON and nothing else; no escape
//!   sequence, no box drawing, no spinner. A pipe receives a stream a machine
//!   parses, which is the whole point of the mode.
//!
//! Records other than the two above — the `system/init` opener, the echoed
//! user turn, `usage`, `tool_result`, `error` and the closing `result` — are
//! lifecycle: the caller may ignore them, and the protocol says it may.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::stream::StreamEvent;

/// The one format name this build speaks.
pub const STREAM_JSON: &str = "stream-json";

/// A headless run's output format (`--output-format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Newline-delimited JSON records (`--output-format stream-json`).
    StreamJson,
}

impl OutputFormat {
    /// Parse a `--output-format` value. Unknown values come back as the
    /// message the usage error prints, naming what is supported.
    ///
    /// # Errors
    /// A model-facing-grammar message (the caller prints it as a usage error)
    /// when the value names no format this build speaks.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            STREAM_JSON => Ok(Self::StreamJson),
            other => Err(format!(
                "invalid output format \"{other}\" (use {STREAM_JSON})"
            )),
        }
    }

    /// The value as the flag spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        STREAM_JSON
    }
}

/// Writes one JSON record per line and flushes after each — a voice caller
/// must be able to speak a delta the moment it lands, so nothing is batched.
pub struct JsonlWriter<W: Write> {
    out: W,
}

impl<W: Write> JsonlWriter<W> {
    #[must_use]
    pub fn new(out: W) -> Self {
        Self { out }
    }

    /// Write one record as one line, then flush.
    ///
    /// # Errors
    /// Whatever the underlying writer reports.
    pub fn write(&mut self, record: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.out, record)?;
        self.out.write_all(b"\n")?;
        self.out.flush()
    }

    /// The writer back, for tests and for a caller that owns the buffer.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.out
    }
}

/// The first record of a run: who answered, where, and under what id.
#[must_use]
pub fn init_record(session_id: &str, model: &str, cwd: &Path) -> Value {
    json!({
        "type": "system",
        "subtype": "init",
        "session_id": session_id,
        "model": model,
        "cwd": cwd.display().to_string(),
    })
}

/// The echoed user turn — what the caller said, so both sides of the
/// conversation are in the stream.
#[must_use]
pub fn user_record(text: &str) -> Value {
    json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
    })
}

/// One turn's records, folded from the same [`StreamEvent`]s the TUI folds
/// into `App`: assistant text, tool calls and the turn's result.
///
/// The segmentation is the App's own (`docs/tool-groups.md` — text before a
/// tool call is finalised ahead of it), so an assistant record never spans a
/// call: what a caller hears in one message is one contiguous run of model
/// text.
#[derive(Debug, Default)]
pub struct TurnRecords {
    /// The text of each completed assistant segment, in order.
    segments: Vec<String>,
    /// The text of the segment still streaming.
    pending: String,
    /// The round's wire call ids, in order — a `ToolStart` carries no id, so
    /// it takes the next one from here.
    calls: VecDeque<String>,
    /// The id of the call currently running, for its result record.
    running: Option<String>,
    /// How many tool calls this turn announced with no wire id to use.
    minted: usize,
    /// The turn's failure text, when one arrived.
    error: Option<String>,
}

impl TurnRecords {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one backend event into records to emit, in order. Text becomes a
    /// delta immediately and is banked for the segment's assistant record;
    /// a tool call is announced as a `tool_use` block and never as text.
    #[must_use]
    pub fn on_event(&mut self, event: &StreamEvent) -> Vec<Value> {
        match event {
            StreamEvent::Chunk(text) => {
                self.pending.push_str(text);
                vec![json!({
                    "type": "stream_event",
                    "event": {
                        "type": "content_block_delta",
                        "delta": {"type": "text_delta", "text": text},
                    },
                })]
            }
            StreamEvent::RoundCalls(calls) => {
                self.calls.extend(calls.iter().map(|call| call.id.clone()));
                Vec::new()
            }
            // A batch announcement, and each call's start, is a segment
            // boundary — the App's own flush point.
            StreamEvent::ToolBatch(_) => self.flush(),
            StreamEvent::ToolStart { name, .. } => {
                let mut out = self.flush();
                let id = self.calls.pop_front().unwrap_or_else(|| {
                    self.minted += 1;
                    format!("toolu_headless_{}", self.minted)
                });
                out.push(json!({
                    "type": "stream_event",
                    "event": {
                        "type": "content_block_start",
                        "content_block": {"type": "tool_use", "id": id, "name": name},
                    },
                }));
                self.running = Some(id);
                out
            }
            StreamEvent::ToolEnd { ok, .. } => self.resolve(!ok),
            StreamEvent::ToolRejected { .. } => self.resolve(true),
            StreamEvent::ToolAnswered { .. } | StreamEvent::ToolBackgrounded { .. } => {
                self.resolve(false)
            }
            StreamEvent::TaskCall { name, ok, .. } => {
                // A task call renders no cell in the TUI either: it is a
                // lifecycle fact, typed, carrying no text.
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": format!("task_{name}"),
                    "is_error": !ok,
                })]
            }
            StreamEvent::Usage(usage) => vec![json!({
                "type": "usage",
                "input_tokens": usage.input,
                "output_tokens": usage.output,
                "cached_input_tokens": usage.cached,
            })],
            StreamEvent::Retrying { attempt, max } => vec![json!({
                "type": "system",
                "subtype": "retry",
                "attempt": attempt,
                "max": max,
            })],
            StreamEvent::Error(message) => {
                self.error = Some(message.clone());
                let mut out = self.flush();
                out.push(json!({
                    "type": "error",
                    "error": {"type": "backend_error", "message": message},
                }));
                out
            }
            // The turn is over: the last run of text is a message like any
            // other, and it must land before the result record. The App's own
            // boundary (`App::finish_stream`).
            StreamEvent::StreamDone => self.flush(),
            // Lifecycle and live-only events: a thinking phase, a call being
            // generated, live tool output, steering, notices. None of it is
            // something a caller should hear, and none of it is text.
            StreamEvent::PromptBlocked { .. }
            | StreamEvent::HookNote { .. }
            | StreamEvent::Steered { .. }
            | StreamEvent::NoticeDelivered { .. }
            | StreamEvent::AgentBatch { .. }
            | StreamEvent::AgentGroupDone { .. }
            | StreamEvent::ThinkingStart
            | StreamEvent::ThinkingChunk(_)
            | StreamEvent::ThinkingEnd
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::ToolNote(_)
            | StreamEvent::ToolOutput(_)
            | StreamEvent::ToolScreen { .. }
            | StreamEvent::ToolTitle(_)
            | StreamEvent::Offline { .. }
            | StreamEvent::Permission(_)
            | StreamEvent::AskUser(_) => Vec::new(),
        }
    }

    /// Close the turn: the outstanding assistant record, then the result.
    #[must_use]
    pub fn finish(&mut self, session_id: &str) -> Vec<Value> {
        let mut out = self.flush();
        let failed = self.error.is_some();
        out.push(json!({
            "type": "result",
            "subtype": if failed { "error" } else { "success" },
            "is_error": failed,
            "result": self.text(),
            "session_id": session_id,
        }));
        out
    }

    /// Every assistant segment this turn produced, joined as one answer.
    #[must_use]
    pub fn text(&self) -> String {
        self.segments.join("\n\n")
    }

    /// The turn's failure text, when the backend reported one.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Finalise the streaming segment as an assistant record — unless it is
    /// empty or whitespace-only, the App's own rule (`App::flush_streaming_segment`):
    /// a run of blanks renders nothing, so it is not an answer.
    fn flush(&mut self) -> Vec<Value> {
        if self.pending.trim().is_empty() {
            self.pending.clear();
            return Vec::new();
        }
        let text = std::mem::take(&mut self.pending);
        let record = assistant_record(&text);
        self.segments.push(text);
        vec![record]
    }

    /// Resolve the running call with a `tool_result` — typed, text-free, so a
    /// caller reading text blocks skips it.
    fn resolve(&mut self, is_error: bool) -> Vec<Value> {
        self.running.take().map_or_else(Vec::new, |id| {
            vec![json!({
                "type": "tool_result",
                "tool_use_id": id,
                "is_error": is_error,
            })]
        })
    }
}

/// `{"type":"assistant","message":{"role":"assistant","content":[…text…]}}` —
/// the record the caller speaks from.
fn assistant_record(text: &str) -> Value {
    json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{RoundCall, TokenUsage};

    /// The writer's whole output, as text.
    fn written(records: &[Value]) -> String {
        let mut out = Vec::new();
        {
            let mut writer = JsonlWriter::new(&mut out);
            for record in records {
                writer.write(record).expect("a writable buffer");
            }
        }
        String::from_utf8(out).expect("UTF-8")
    }

    #[test]
    fn a_format_is_parsed_by_name_and_refuses_unknown_ones() {
        assert_eq!(
            OutputFormat::parse("stream-json"),
            Ok(OutputFormat::StreamJson)
        );
        assert_eq!(OutputFormat::StreamJson.as_str(), "stream-json");
        let err = OutputFormat::parse("yaml").unwrap_err();
        assert!(err.contains("yaml") && err.contains("stream-json"), "{err}");
    }

    #[test]
    fn the_writer_puts_one_record_on_each_line_and_ends_with_a_newline() {
        let text = written(&[json!({"a": 1}), json!({"b": "two"})]);
        assert_eq!(text, "{\"a\":1}\n{\"b\":\"two\"}\n");
        // Every line decodes on its own — the NDJSON contract.
        for line in text.lines() {
            serde_json::from_str::<Value>(line).expect("a decodable line");
        }
    }

    #[test]
    fn a_record_with_a_newline_in_its_text_still_occupies_one_line() {
        let text = written(&[user_record("one\ntwo")]);
        assert_eq!(text.lines().count(), 1, "{text:?}");
        let record: Value = serde_json::from_str(text.trim_end()).unwrap();
        assert_eq!(record["message"]["content"][0]["text"], "one\ntwo");
    }

    #[test]
    fn the_two_contract_shapes_are_exactly_the_documented_ones() {
        let user = user_record("hi");
        assert_eq!(user["type"], "user");
        assert_eq!(user["message"]["role"], "user");
        assert_eq!(user["message"]["content"][0]["type"], "text");

        let mut turn = TurnRecords::new();
        let delta = turn.on_event(&StreamEvent::Chunk("hello".to_string()));
        assert_eq!(
            delta[0],
            json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_delta",
                    "delta": {"type": "text_delta", "text": "hello"},
                },
            })
        );
        let assistant = turn.finish("s");
        assert_eq!(
            assistant[0],
            json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{"type": "text", "text": "hello"}],
                },
            })
        );
    }

    #[test]
    fn text_before_a_call_is_finalised_ahead_of_it() {
        let mut turn = TurnRecords::new();
        let _ = turn.on_event(&StreamEvent::Chunk("First.".to_string()));
        let _ = turn.on_event(&StreamEvent::Chunk(" More.".to_string()));
        let at_batch = turn.on_event(&StreamEvent::ToolBatch(Vec::new()));
        assert_eq!(at_batch.len(), 1);
        assert_eq!(at_batch[0]["type"], "assistant");
        assert_eq!(at_batch[0]["message"]["content"][0]["text"], "First. More.");
        // The call itself is a tool_use block, with the round's own id.
        let _ = turn.on_event(&StreamEvent::RoundCalls(vec![RoundCall {
            id: "call_1".to_string(),
            name: "Read".to_string(),
            arguments: "{}".to_string(),
        }]));
        let started = turn.on_event(&StreamEvent::ToolStart {
            name: "Read".to_string(),
            args: "a.txt".to_string(),
            detail: None,
            arguments: Some("{\"file_path\":\"a.txt\"}".to_string()),
        });
        assert_eq!(started[0]["event"]["content_block"]["type"], "tool_use");
        assert_eq!(started[0]["event"]["content_block"]["id"], "call_1");
        assert_eq!(started[0]["event"]["content_block"]["name"], "Read");
        // Its resolution is typed, text-free.
        let ended = turn.on_event(&StreamEvent::ToolEnd {
            output: "secret output".to_string(),
            ok: true,
            truncated: false,
        });
        assert_eq!(ended[0]["type"], "tool_result");
        assert_eq!(ended[0]["tool_use_id"], "call_1");
        assert_eq!(ended[0]["is_error"], false);
        assert!(!ended[0].to_string().contains("secret output"));
        // The tail of the reply is its own message.
        let _ = turn.on_event(&StreamEvent::Chunk("Then.".to_string()));
        let tail = turn.on_event(&StreamEvent::StreamDone);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0]["message"]["content"][0]["text"], "Then.");
        assert_eq!(turn.text(), "First. More.\n\nThen.");
    }

    #[test]
    fn a_call_that_never_streamed_its_id_still_gets_a_typed_block() {
        let mut turn = TurnRecords::new();
        let started = turn.on_event(&StreamEvent::ToolStart {
            name: "Bash".to_string(),
            args: "ls".to_string(),
            detail: None,
            arguments: None,
        });
        let block = &started[0]["event"]["content_block"];
        assert_eq!(block["type"], "tool_use");
        assert!(
            block["id"].as_str().is_some_and(|id| !id.is_empty()),
            "an id is minted: {block:?}"
        );
    }

    #[test]
    fn thinking_and_live_output_are_never_text() {
        let mut turn = TurnRecords::new();
        for event in [
            StreamEvent::ThinkingStart,
            StreamEvent::ThinkingChunk("the reasoning".to_string()),
            StreamEvent::ThinkingEnd,
            StreamEvent::ToolOutput("live line".to_string()),
            StreamEvent::ToolScreen {
                settled: "settled".to_string(),
                live: "live".to_string(),
            },
            StreamEvent::ToolNote("a note".to_string()),
            StreamEvent::ToolTitle("a title".to_string()),
        ] {
            assert!(
                turn.on_event(&event).is_empty(),
                "{event:?} is not something to speak"
            );
        }
        assert_eq!(turn.text(), "");
    }

    #[test]
    fn usage_retry_and_failure_are_lifecycle_records_with_no_speech() {
        let mut turn = TurnRecords::new();
        let usage = turn.on_event(&StreamEvent::Usage(TokenUsage {
            input: 10,
            output: 4,
            cached: 6,
            cache_write: 0,
            reasoning: 0,
        }));
        assert_eq!(usage[0]["type"], "usage");
        assert_eq!(usage[0]["input_tokens"], 10);
        let retry = turn.on_event(&StreamEvent::Retrying { attempt: 2, max: 3 });
        assert_eq!(retry[0]["subtype"], "retry");
        let _ = turn.on_event(&StreamEvent::Chunk("half a reply".to_string()));
        let failed = turn.on_event(&StreamEvent::Error("boom".to_string()));
        assert_eq!(failed[0]["type"], "assistant");
        assert_eq!(failed[1]["type"], "error");
        assert_eq!(failed[1]["error"]["message"], "boom");
        let result = turn.finish("s");
        assert_eq!(result[0]["subtype"], "error");
        assert_eq!(result[0]["is_error"], true);
        assert_eq!(result[0]["result"], "half a reply");
        assert_eq!(turn.error(), Some("boom"));
    }

    #[test]
    fn a_blank_segment_is_not_an_answer() {
        let mut turn = TurnRecords::new();
        let _ = turn.on_event(&StreamEvent::Chunk("\n\n".to_string()));
        assert!(turn.on_event(&StreamEvent::ToolBatch(Vec::new())).is_empty());
        assert_eq!(turn.text(), "");
    }

    #[test]
    fn the_init_record_names_the_session_model_and_directory() {
        let record = init_record("abc", "a-model", Path::new("/w"));
        assert_eq!(record["type"], "system");
        assert_eq!(record["subtype"], "init");
        assert_eq!(record["session_id"], "abc");
        assert_eq!(record["model"], "a-model");
        assert_eq!(record["cwd"], "/w");
    }
}
