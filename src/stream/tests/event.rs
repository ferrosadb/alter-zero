//! The reply protocol's own vocabulary ([`crate::stream::StreamEvent`]).

use super::*;
use crate::stream::{TokenUsage, ToolCallFragment, ToolCallSummary};

#[test]
fn only_what_a_streaming_request_emits_is_attempt_output() {
    // What a retry voids (docs/offline.md *A connection lost mid-reply*):
    // the reply text, a thinking phase, a call the model is still generating
    // — everything a request emits while it streams, and nothing that marks
    // where one stopped or the next began.
    let output = [
        StreamEvent::Chunk("Half a rep".into()),
        StreamEvent::ThinkingStart,
        StreamEvent::ThinkingChunk("weighing".into()),
        StreamEvent::ThinkingEnd,
        StreamEvent::ToolCallDelta(ToolCallFragment {
            index: 0,
            name: "write".into(),
            arguments: "{\"pa".into(),
        }),
    ];
    for event in output {
        assert!(event.is_attempt_output(), "{event:?} is attempt output");
    }
    let boundaries = [
        StreamEvent::Retrying { attempt: 1, max: 3 },
        StreamEvent::Offline {
            host: "api.venice.ai".into(),
            attempts: 1,
        },
        StreamEvent::Usage(TokenUsage::default()),
        StreamEvent::RoundCalls(Vec::new()),
        StreamEvent::ToolBatch(vec![ToolCallSummary {
            name: "Bash".into(),
            args: "ls".into(),
        }]),
        StreamEvent::ToolStart {
            name: "Bash".into(),
            args: "ls".into(),
            detail: None,
            arguments: None,
        },
        StreamEvent::ToolEnd {
            output: "src".into(),
            ok: true,
            truncated: false,
        },
        StreamEvent::Steered {
            text: "and the tests".into(),
        },
        StreamEvent::NoticeDelivered { seq: 1 },
        StreamEvent::HookNote {
            label: "Stop hook".into(),
            text: "keep going".into(),
            report: None,
        },
        StreamEvent::Error("boom".into()),
        StreamEvent::StreamDone,
    ];
    for event in boundaries {
        assert!(
            !event.is_attempt_output(),
            "{event:?} is not attempt output"
        );
    }
}

#[test]
fn only_what_draws_or_records_settles_an_ended_thought() {
    // A thought whose phase ended keeps its live block up until whatever
    // takes its rows arrives (docs/handoff-hold.md *The thought lingers*).
    // What leaves it up draws nothing in the strip and records nothing: the
    // pieces of the call being written (its cell lands only with its batch),
    // blank reply text, the round's usage frame and call ids, and a retry —
    // which voids the attempt the thought belongs to instead.
    let lingers = [
        StreamEvent::ToolCallDelta(ToolCallFragment {
            index: 0,
            name: "skill".into(),
            arguments: "{\"sk".into(),
        }),
        StreamEvent::Chunk(" \n".into()),
        StreamEvent::Usage(TokenUsage::default()),
        StreamEvent::RoundCalls(Vec::new()),
        StreamEvent::Retrying { attempt: 1, max: 3 },
    ];
    for event in lingers {
        assert!(
            !event.settles_ended_thought(),
            "{event:?} leaves an ended thought's block up"
        );
    }
    // Anything that draws or records something settles it first, so its
    // `Thought for …` line commits ahead of what the event commits.
    let settles = [
        StreamEvent::Chunk("The champion is".into()),
        StreamEvent::ToolBatch(vec![ToolCallSummary {
            name: "Skill".into(),
            args: "web-search".into(),
        }]),
        StreamEvent::ToolStart {
            name: "Bash".into(),
            args: "ls".into(),
            detail: None,
            arguments: None,
        },
        StreamEvent::ThinkingStart,
        StreamEvent::ThinkingChunk("again".into()),
        StreamEvent::Steered {
            text: "and the tests".into(),
        },
        StreamEvent::NoticeDelivered { seq: 1 },
        StreamEvent::HookNote {
            label: "Stop hook".into(),
            text: "keep going".into(),
            report: None,
        },
        StreamEvent::Offline {
            host: "api.venice.ai".into(),
            attempts: 1,
        },
        StreamEvent::Error("boom".into()),
        StreamEvent::StreamDone,
    ];
    for event in settles {
        assert!(
            event.settles_ended_thought(),
            "{event:?} settles an ended thought first"
        );
    }
}
