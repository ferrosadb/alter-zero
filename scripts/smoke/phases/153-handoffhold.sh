#!/usr/bin/env bash
# Phase 153 — the box holds still while one live cell hands over to the next, and held rows never outlive the hand-off

. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
smoke_begin

# THE HAND-OFF HOLD (docs/handoff-hold.md). The live region is
# content-anchored, so a frame whose strip sheds more rows than it commits
# lifts the box — and every hand-off between live cells did: a thought settled
# into its one-line `Thought for …` cell before the round's call had a cell
# (the reported flicker between a quick think and a `skill` call), a read
# group's two-row cell settled into its line under the edit behind it. The box
# rose and fell back a few frames later.
#
# Two answers, and this phase holds each to what it promises:
#   - The THOUGHT lingers: its `● Thinking…` block stays up, unchanged, until
#     the call's cell takes its rows in one frame — no blank rows reserved over
#     the status line, and never the settled line over its stale thought (the
#     two looks earlier fixes were turned down for).
#   - Every other hand-off HOLDS the rows it gave up as blank rows over the
#     status line, while the next cell is on its way and not a moment longer:
#     rows held until the next REQUEST answered sat as empty lines over the
#     status line for the whole round boundary — the first fix, rejected.
#
# Each turn is sampled ~20 times a second, every candidate confirmed against a
# second capture a beat later (Phase 121's rule: a torn frame lasts
# microseconds, a hop or a held gap as long as the hand-off). Three turns:
#   1. the default dummy turn — a paragraph, a two-line thought, then a
#      Read+Edit+Bash batch generated over about a second — with Verbose
#      output off, so the read folds into a group that closes under the edit;
#      on a terminal with room below the box, its byte stream checked for
#      purge rebuilds and its cells for landing exactly once;
#   2. the same turn on a short terminal, where the box sits on the bottom row
#      and a rise strands blank rows under it;
#   3. the REAL backend against a paced stub provider: a three-line thought,
#      a `bash` call written over about a second, a command whose live tail
#      stands taller than its settled peek, then a three-second round
#      boundary before the stub answers.
S153="${S}_handoff"
S153S="${S}_handoffshort"
RAW153="$SMOKE_TMP/phase153.raw"
# The dummy turn with Verbose output off: the read joins a group that closes
# under the edit (docs/tool-groups.md).
APP153="env $CFG_ENV_NOHIST ALTER_ZERO_VERBOSE=0 ALTER_ZERO_STARTUP_DELAY_MS=$SMOKE_STARTUP_MS $BIN"

# The blank rows directly above the status line in a capture, or nothing when
# no status line is on screen.
gap_above_status() {
	printf '%s\n' "$1" | awk '
		/esc to interrupt/ { print blank; exit }
		/^[[:space:]]*$/ { blank++; next }
		{ blank = 0 }'
}

# The last screen row holding FOOTER, or nothing.
footer_row() {
	printf '%s\n' "$1" | grep -nF "$2" | tail -1 | cut -d: -f1
}

# watch_turn SESSION FOOTER THOUGHT LANDED [SETTLED] — sample a turn until its
# status line goes. Sets:
#   W_SAMPLES        how many captures the watch took;
#   W_HOPS           confirmed rises of the FOOTER row while the status line
#                    was up — until SETTLED appears, when SETTLED is given (a
#                    command that settles shorter than it ran lets the box
#                    rise once its hold lets go between rounds: by design);
#   W_LINGER_GAP_MS  the longest confirmed run, in ms, of samples with more
#                    than one blank row above the status line BEFORE LANDED
#                    appears — the thought's hand-off, where its block lingers
#                    instead: no rows are reserved there;
#   W_GAP_MS         the same after LANDED — or once SETTLED is on screen, when
#                    SETTLED is given (the round boundary);
#   W_STALE          confirmed samples showing the settled `Thought for` line
#                    with THOUGHT still on screen under it.
watch_turn() {
	local sess="$1" footer="$2" thought="$3" landed="$4" settled="${5:-}"
	local vis again now now_again prev=0 seen=0 gap gap_again
	local t0 t run_start=0 in_run=0 phase=0 hops_open=1
	W_SAMPLES=0
	W_HOPS=0
	W_LINGER_GAP_MS=0
	W_GAP_MS=0
	W_STALE=0
	t0=$(date +%s%3N)
	for _ in $(seq 1 600); do # up to ~30s
		vis="$(tmux capture-pane -t "$sess" -p)"
		t=$(($(date +%s%3N) - t0))
		W_SAMPLES=$((W_SAMPLES + 1))
		if printf '%s' "$vis" | grep -qF "esc to interrupt"; then
			seen=1
		elif [ "$seen" -eq 1 ]; then
			# The turn is over: its end collapses the strip into the summary,
			# which is a settle rather than a hop.
			break
		fi
		# Which stretch of the turn this sample belongs to: 0 until the call's
		# cell lands, 1 after, 2 once SETTLED is on screen.
		if [ "$phase" -eq 0 ] && printf '%s' "$vis" | grep -qF "$landed"; then
			phase=1
			in_run=0
		fi
		if [ -n "$settled" ] && [ "$phase" -lt 2 ] && printf '%s' "$vis" | grep -qF "$settled"; then
			phase=2
			hops_open=0
			in_run=0
		fi
		if printf '%s' "$vis" | grep -qE '^Thought for [0-9]+s' &&
			printf '%s' "$vis" | grep -qF "$thought"; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			if printf '%s' "$again" | grep -qE '^Thought for [0-9]+s' &&
				printf '%s' "$again" | grep -qF "$thought"; then
				W_STALE=$((W_STALE + 1))
				if [ "$W_STALE" -le 1 ]; then
					dump "$sess: the settled line stands over its stale thought (sample $W_SAMPLES, ${t}ms)" "$again"
				fi
			fi
		fi
		now="$(footer_row "$vis" "$footer")"
		if [ "$hops_open" -eq 1 ] && [ "${now:-0}" -gt 0 ] && [ "$now" -lt "$prev" ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			now_again="$(footer_row "$again" "$footer")"
			if printf '%s' "$again" | grep -qF "esc to interrupt" &&
				[ "${now_again:-0}" -gt 0 ] && [ "$now_again" -lt "$prev" ]; then
				W_HOPS=$((W_HOPS + 1))
				if [ "$W_HOPS" -le 2 ]; then
					dump "$sess: the box hopped up from row $prev to row $now_again (sample $W_SAMPLES, ${t}ms)" "$again"
				fi
			else
				now="${now_again:-$now}"
			fi
		fi
		prev="${now:-$prev}"
		# Gaps count before the cell lands (the linger) and after it — or only
		# from SETTLED on, when given (the round boundary).
		if [ "$phase" -eq 0 ] || [ -z "$settled" ] || [ "$phase" -eq 2 ]; then
			gap="$(gap_above_status "$vis")"
			if [ "${gap:-1}" -gt 1 ]; then
				sleep 0.03
				again="$(tmux capture-pane -t "$sess" -p)"
				gap_again="$(gap_above_status "$again")"
				if [ "${gap_again:-1}" -gt 1 ]; then
					if [ "$in_run" -eq 0 ]; then
						in_run=1
						run_start=$t
					fi
					if [ "$phase" -eq 0 ]; then
						[ $((t - run_start)) -gt "$W_LINGER_GAP_MS" ] && W_LINGER_GAP_MS=$((t - run_start))
						if [ "$W_LINGER_GAP_MS" -eq 0 ]; then
							W_LINGER_GAP_MS=1 # a confirmed gap, however short
							dump "$sess: $gap_again blank rows above the status line before the call landed (${t}ms)" "$again"
						fi
					else
						[ $((t - run_start)) -gt "$W_GAP_MS" ] && W_GAP_MS=$((t - run_start))
						if [ "$W_GAP_MS" -gt 1500 ]; then
							dump "$sess: $gap_again blank rows above the status line for ${W_GAP_MS}ms" "$again"
						fi
					fi
				else
					in_run=0
				fi
			else
				in_run=0
			fi
		fi
		sleep 0.05
	done
	[ "$seen" -eq 1 ] || fail "$sess: the turn never showed its status line"
	if [ "$W_SAMPLES" -lt 20 ]; then
		fail "$sess: only $W_SAMPLES samples of the turn: the watch saw too little to mean anything"
	fi
	if [ "$phase" -eq 0 ]; then
		fail "$sess: the call's cell ('$landed') never landed: the watch saw no hand-off"
	fi
}

# expect_watch WHAT — the verdicts on the last watch_turn.
expect_watch() {
	if [ "$W_HOPS" -ne 0 ]; then
		fail "$1: the box hopped up $W_HOPS time(s) mid-hand-off: the strip gave its rows up before the next cell could take them (see the frames above)"
	fi
	if [ "$W_LINGER_GAP_MS" -ne 0 ]; then
		fail "$1: blank rows stood over the status line before the call's cell landed: the thought's block gave its rows up instead of lingering (see the frame above)"
	fi
	if [ "$W_STALE" -ne 0 ]; then
		fail "$1: the settled 'Thought for' line stood over its stale thought (see the frame above)"
	fi
	if [ "$W_GAP_MS" -gt 1500 ]; then
		fail "$1: blank rows sat over the status line for ${W_GAP_MS}ms: a hold outlived its hand-off"
	fi
}

# ---- 1. the dummy turn, with room below the box ------------------------------
launch "$S153" 80 24 "$APP153"
tmux pipe-pane -t "$S153" -o "cat > $RAW153"
submit "$S153" "$USER_MSG"
watch_turn "$S153" 'dummy_model_name ·' "Let me read the file first." "Reading 1 file"
note "tall: sampled $W_SAMPLES times; box hops=$W_HOPS; gap while lingering=${W_LINGER_GAP_MS}ms; longest held gap=${W_GAP_MS}ms; stale=$W_STALE"
expect_watch "tall"
wait_summaries 20 "$S153" 1 >/dev/null || fail "the default turn never finished"
tmux pipe-pane -t "$S153" # close the recording before quitting

final="$(pane "$S153" -S -200)"
dump "captured pane + scrollback (the settled turn)" "$final"
# Liveness: the turn reached both hand-offs, so a quiet watch is a real pass.
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$final")" "1" \
	"the thought did not settle into exactly one 'Thought for' line"
expect_eq "$(grep -cE '^Read 1 file' <<<"$final")" "1" "the read group's line is not there exactly once"
expect_eq "$(grep -cF '● Edit(about.py)' <<<"$final")" "1" "the edit's cell is not there exactly once"
expect_eq "$(grep -cF '● Bash(python3 about.py)' <<<"$final")" "1" "the run's cell is not there exactly once"
purges=$(grep -c $'\x1b\\[3J' "$RAW153" 2>/dev/null || true)
expect_eq "${purges:-0}" "0" "the turn purge-rebuilt the screen"
keys "$S153" C-c
wait_gone 10 "$S153" || fail "the app did not quit on Ctrl+C"

# ---- 2. the dummy turn on a short terminal: the box on the bottom row --------
launch "$S153S" 80 16 "$APP153"
submit "$S153S" "$USER_MSG"
watch_turn "$S153S" 'dummy_model_name ·' "Let me read the file first." "Reading 1 file"
note "short: sampled $W_SAMPLES times; box hops=$W_HOPS; gap while lingering=${W_LINGER_GAP_MS}ms; longest held gap=${W_GAP_MS}ms; stale=$W_STALE"
expect_watch "short"
short="$(wait_summaries 20 "$S153S" 1)" || fail "the short session's turn never finished"
# Settled, the box is flush at the bottom: nothing held outlived the turn.
footer_at="$(footer_row "$short" 'dummy_model_name ·')"
expect_eq "${footer_at:-0}" "16" "the settled box is not on the bottom row: a held row outlived the turn"
expect_eq "$(count_footers "$short")" "1" "the settled short session shows the footer more than once"
keys "$S153S" C-c
wait_gone 10 "$S153S" || fail "the short session did not quit on Ctrl+C"

# ---- 3. the real backend against a paced stub provider ----------------------
# The dummy scripts its events; this turn goes through `LlmBackend` and
# `stream_round` the way a provider's stream does: a three-line thought, then a
# `bash` call whose arguments stream over about a second (the thought settles
# at its first piece, its cell lands only when the round ends — the hold must
# wait for it), a command that prints thirty lines so its live tail stands
# taller than its settled peek, and a three-second wait before the second
# round: the round boundary the first fix filled with empty lines.
SR_PORT="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 - "$SR_PORT" <<'PY' &
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port = int(sys.argv[1])

class Stub(BaseHTTPRequestHandler):
    def do_GET(self):
        data = b'{"object":"list","data":[{"id":"stub-model"}]}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def emit(self, delta, pause=0.0, finish=None):
        choice = {"delta": delta}
        if finish:
            choice["finish_reason"] = finish
        self.wfile.write(("data: %s\n\n" % json.dumps({"choices": [choice]})).encode())
        self.wfile.flush()
        time.sleep(pause)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length") or 0)))
        messages = body.get("messages", [])
        first_round = messages[-1].get("role") == "user"
        # HTTP/1.0 with no length: the stream ends when the connection does,
        # so each frame reaches the client as it is written.
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        if first_round:
            for line in ["Let me run the command first.\n", "It prints thirty lines.\n",
                         "Then I can answer.\n"]:
                self.emit({"reasoning_content": line}, 0.15)
            command = "for i in $(seq 1 30); do echo line-$i; sleep 0.05; done"
            args = json.dumps({"command": command, "description": "Print thirty lines"})
            pieces = [args[i:i + 12] for i in range(0, len(args), 12)]
            self.emit({"tool_calls": [{"index": 0, "id": "call_153", "type": "function",
                                       "function": {"name": "bash", "arguments": pieces[0]}}]}, 0.12)
            for piece in pieces[1:]:
                self.emit({"tool_calls": [{"index": 0, "function": {"arguments": piece}}]}, 0.12)
            self.emit({}, finish="tool_calls")
        else:
            time.sleep(3)  # the round boundary
            self.emit({"content": "Done."})
            self.emit({}, finish="stop")
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def log_message(self, *_args):
        pass

ThreadingHTTPServer(("127.0.0.1", port), Stub).serve_forever()
PY
SR_STUB=$!
smoke_on_exit kill "$SR_STUB"
for _ in $(seq 1 30); do # wait for the stub to listen
	if python3 -c "import socket, sys; s = socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(('127.0.0.1', $SR_PORT)) == 0 else 1)" 2>/dev/null; then
		break
	fi
	sleep 0.1
done
SR_PROVIDERS="$SMOKE_TMP/providers153.toml"
printf '[providers.stub]\nname = "Stub"\n\n[providers.stub.kwargs]\napi_base = "http://127.0.0.1:%s/v1"\n' "$SR_PORT" >"$SR_PROVIDERS"
# Master mode: the command runs unasked, so nothing but the strip is on screen.
SR_APP="env NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 $CFG_ENV_NOHIST ALTER_ZERO_PERMISSION_MODE=master ALTER_ZERO_PROVIDERS_FILE=$SR_PROVIDERS ALTER_ZERO_PROVIDER=stub ALTER_ZERO_MODEL=stub-model STUB_API_KEY=stand-in $BIN_ABS"

S153R="${S}_handoffreal"
launch -c "$(work_dir real)" "$S153R" 100 30 "$SR_APP"
submit "$S153R" "run the command"
# The thought lingers while the call is written (no rows reserved), the cell
# lands, the command runs and settles into its peek (`(ctrl+o to expand)`
# under its cell); held gaps count from there — the round boundary, three
# seconds long, where the hold lets go after 300ms (1.5s is generous slack
# for a loaded machine, and still well short of the whole wait the rejected
# floor held).
watch_turn "$S153R" 'stub-model ·' "Let me run the command first." "● Bash(" "lines (ctrl+o to expand)"
note "real backend: sampled $W_SAMPLES times; hops=$W_HOPS; gap while lingering=${W_LINGER_GAP_MS}ms; longest held gap at the round boundary=${W_GAP_MS}ms; stale=$W_STALE"
expect_watch "real backend"
real="$(wait_pane 20 "$S153R" -S -60 -- -F "Done.")" || fail "the stub's second round never answered"
dump "the settled real-backend session" "$real"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$real")" "1" \
	"the stub's thought did not settle into exactly one 'Thought for' line"
expect_eq "$(grep -cF '● Bash(for i in' <<<"$real")" "1" "the stub's call is not there exactly once"
expect_lacks "$real" -F "Let me run the command first." "the stub's thought reached the conversation: only its line may commit"
keys "$S153R" C-c
wait_gone 10 "$S153R" || fail "the real-backend session did not quit on Ctrl+C"
