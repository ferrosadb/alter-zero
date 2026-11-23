//! The headless machine-readable stream (`--output-format claude-stream-json`),
//! driven end to end through the **real** agent loop and the crate's own
//! offline backend (`ALTER_ZERO_DUMMY=1`) — no fake, no shortcut around `App` or the
//! backend seam. The caller is a phone voice agent, so these tests hold the
//! contract it parses: assistant text arrives as an assistant message and as
//! streamed text deltas, a tool call never does, and stdout is pure NDJSON.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use alter_zero::stream::MARKDOWN_TOUR;

/// A scratch directory per run, removed when the run is read.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "alter-zero-claude-stream-{}-{nanos}",
            std::process::id()
        ));
        for dir in ["cfg", "sessions", "home", "skills", "agents", "work"] {
            std::fs::create_dir_all(path.join(dir)).expect("a scratch dir");
        }
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the real binary as `-p --output-format claude-stream-json` against the
/// offline demo backend, its pacing at zero, `stdin` piped as further turns,
/// and return everything it wrote to stdout.
fn run(prompt: Option<&str>, stdin: Option<&str>) -> String {
    let scratch = Scratch::new();
    let dir = |name: &str| scratch.0.join(name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_alter-zero"));
    command
        .args(["-p", "--output-format", "claude-stream-json"])
        .args(prompt)
        .current_dir(dir("work"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir("home"))
        .env("ALTER_ZERO_CONFIG_DIR", dir("cfg"))
        .env("ALTER_ZERO_SESSIONS_DIR", dir("sessions"))
        .env("ALTER_ZERO_SKILLS_DIR", dir("skills"))
        .env("ALTER_ZERO_AGENTS_DIR", dir("agents"))
        .env("ALTER_ZERO_HISTORY_FILE", "/dev/null")
        .env("ALTER_ZERO_PROJECT_CONFIG", "0")
        .env("ALTER_ZERO_CHECKPOINTS", "0")
        .env("ALTER_ZERO_TELEMETRY", "0")
        .env("ALTER_ZERO_UPDATE_CHECK", "0")
        .env("ALTER_ZERO_STARTUP_DELAY_MS", "0")
        .env("ALTER_ZERO_CHUNK_DELAY_MS", "0")
        .env("ALTER_ZERO_DUMMY", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the binary runs");
    {
        let mut pipe = child.stdin.take().expect("a stdin pipe");
        pipe.write_all(stdin.unwrap_or_default().as_bytes())
            .expect("stdin takes the turns");
    }
    let out = child.wait_with_output().expect("the run ends");
    assert!(
        out.status.success(),
        "exit {:?}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout is UTF-8")
}

/// Every record of a run, one per line — panicking on a line that does not
/// decode, which is itself the contract (`b`).
fn records(raw: &str) -> Vec<Value> {
    assert!(
        raw.ends_with('\n'),
        "every record ends with a newline: {raw:?}"
    );
    raw.lines()
        .map(|line| {
            assert!(!line.is_empty(), "no blank lines: {raw:?}");
            serde_json::from_str(line).unwrap_or_else(|e| panic!("{line:?} is not JSON: {e}"))
        })
        .collect()
}

/// The text of every `assistant` record, in order.
fn assistant_texts(records: &[Value]) -> Vec<String> {
    records
        .iter()
        .filter(|record| record["type"] == "assistant")
        .map(|record| {
            assert_eq!(record["message"]["role"], "assistant");
            record["message"]["content"]
                .as_array()
                .expect("content is an array")
                .iter()
                .map(|block| block["text"].as_str().unwrap_or_default())
                .collect::<String>()
        })
        .collect()
}

/// Every `tool_use` content block a run announced.
fn tool_use_blocks(records: &[Value]) -> Vec<&Value> {
    records
        .iter()
        .filter(|record| record["event"]["content_block"]["type"] == "tool_use")
        .map(|record| &record["event"]["content_block"])
        .collect()
}

/// (a) A headless turn prints the assistant's answer as the record the caller
/// parses, and its text is exactly what the model produced — the shipped
/// text-only script, character for character.
#[test]
fn a_turn_prints_the_assistants_text_as_one_parseable_record() {
    let raw = run(Some("markdown"), None);
    let records = records(&raw);
    let texts = assistant_texts(&records);
    assert_eq!(
        texts.concat(),
        MARKDOWN_TOUR,
        "the assistant record carries the model's reply verbatim"
    );
    // No tool ran, so the whole answer is one message record.
    assert_eq!(texts.len(), 1, "one text-only reply, one record: {texts:?}");
}

/// (a) The same, streamed: each delta is the record shape the caller reads,
/// and the deltas concatenate to the same answer.
#[test]
fn the_answer_also_arrives_as_streamed_text_deltas() {
    let records = records(&run(Some("markdown"), None));
    let deltas: Vec<&str> = records
        .iter()
        .filter(|record| record["type"] == "stream_event")
        .filter(|record| record["event"]["type"] == "content_block_delta")
        .map(|record| {
            assert_eq!(record["event"]["delta"]["type"], "text_delta");
            record["event"]["delta"]["text"]
                .as_str()
                .expect("a text delta carries text")
        })
        .collect();
    assert!(!deltas.is_empty(), "a reply streams in pieces");
    assert_eq!(deltas.concat(), MARKDOWN_TOUR);
}

/// (b) Every line stdout carries decodes on its own — one record per line, a
/// newline-terminated stream, no pretty-printing and no framing.
#[test]
fn stdout_is_one_json_record_per_line() {
    let raw = run(Some("run these in parallel"), None);
    assert!(raw.lines().count() > 5, "a whole turn is many records");
    for line in raw.lines() {
        assert!(
            serde_json::from_str::<Value>(line).is_ok(),
            "line does not decode: {line:?}"
        );
    }
}

/// (c) Headless stdout never carries a terminal escape or a painted row: no
/// C0 control byte but the record separator, no line opening on a cell's
/// bullet, and no record type outside the protocol. The model's own prose may
/// name a glyph; what must never appear is a *rendered* frame.
#[test]
fn stdout_carries_no_escapes_or_tui_chrome() {
    let raw = run(Some("run these in parallel"), None);
    for byte in raw.bytes() {
        assert!(
            byte == b'\n' || byte >= 0x20,
            "control byte {byte:#04x} on stdout: {raw:?}"
        );
    }
    for line in raw.lines() {
        for chrome in [
            "\u{25cf} ",
            "\u{23bf}",
            "\u{256d}",
            "\u{2570}",
            "\u{2500}\u{2500}",
        ] {
            assert!(!line.starts_with(chrome), "painted row {line:?}");
        }
    }
    const TYPES: [&str; 8] = [
        "system",
        "user",
        "assistant",
        "stream_event",
        "tool_result",
        "usage",
        "error",
        "result",
    ];
    for record in records(&raw) {
        let kind = record["type"].as_str().expect("every record is typed");
        assert!(TYPES.contains(&kind), "unknown record type {kind:?}");
    }
}

/// (d) A tool call in the same turn is announced as a `tool_use` block and
/// never leaks into assistant text — the words a voice caller would speak.
#[test]
fn a_tool_call_is_never_spoken_as_assistant_text() {
    let records = records(&run(Some("run these in parallel"), None));
    let tools = tool_use_blocks(&records);
    assert_eq!(tools.len(), 3, "the parallel turn runs three calls");
    assert!(
        tools.iter().all(|block| block["type"] == "tool_use"),
        "every tool block is typed tool_use: {tools:?}"
    );
    assert!(
        tools
            .iter()
            .all(|block| block.get("text").is_none() && block["name"] == "Bash"),
        "a tool_use block carries a name, never text: {tools:?}"
    );
    // The call's args and output never reach the text the caller speaks.
    let spoken = assistant_texts(&records).concat();
    for leak in [
        "ping -c 20",
        "142.250.72.14",
        "icmp_seq",
        "Exit code:",
        "\"command\"",
    ] {
        assert!(!spoken.contains(leak), "{leak} leaked into {spoken:?}");
    }
    // …and every tool_result is text-free too.
    for record in &records {
        if record["type"] == "tool_result" {
            assert!(
                record.get("result").is_none() && record.get("text").is_none(),
                "a tool_result carries no text: {record}"
            );
        }
    }
}

/// Lines on stdin are further turns of the same conversation: a voice call
/// sends many utterances into one process.
#[test]
fn stdin_lines_become_further_turns_of_the_same_conversation() {
    let records = records(&run(
        Some("one markdown turn"),
        Some("a second markdown turn\na third markdown turn\n"),
    ));
    let users: Vec<&str> = records
        .iter()
        .filter(|record| record["type"] == "user")
        .map(|record| record["message"]["content"][0]["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        users,
        [
            "one markdown turn",
            "a second markdown turn",
            "a third markdown turn"
        ]
    );
    // Three turns, three answers.
    let results = records
        .iter()
        .filter(|record| record["type"] == "result")
        .count();
    assert_eq!(results, 3, "one result per turn");
}

/// A blank stdin line is not a turn — a caller that keeps the pipe open and
/// sends nothing gets no request.
#[test]
fn a_blank_stdin_line_is_not_a_turn() {
    let records = records(&run(Some("only markdown turn"), Some("\n   \n")));
    let users = records
        .iter()
        .filter(|record| record["type"] == "user")
        .count();
    assert_eq!(users, 1);
}

/// The run opens by naming itself — session id, model and cwd — so the caller
/// can resume the conversation it is about to have.
#[test]
fn the_run_starts_with_an_init_record() {
    let records = records(&run(Some("markdown"), None));
    let init = &records[0];
    assert_eq!(init["type"], "system");
    assert_eq!(init["subtype"], "init");
    assert!(init["session_id"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(init["model"], "dummy_model_name");
    assert!(
        init["cwd"]
            .as_str()
            .is_some_and(|cwd| cwd.ends_with("work"))
    );
}

/// A turn that ends cleanly reports success and carries the answer text.
#[test]
fn a_clean_turn_ends_with_a_success_result() {
    let records = records(&run(Some("markdown"), None));
    let result = records.last().expect("a final record");
    assert_eq!(result["type"], "result");
    assert_eq!(result["subtype"], "success");
    assert_eq!(result["is_error"], false);
    assert_eq!(result["session_id"], records[0]["session_id"]);
}
