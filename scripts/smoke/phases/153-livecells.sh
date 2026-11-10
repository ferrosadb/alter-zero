#!/usr/bin/env bash
# Phase 153 — a live cell settles without moving the box: thought → call → command → command, on the dummy and on the real backend

. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
smoke_begin

# LIVE CELLS SETTLE IN PLACE (docs/live-cells.md). The live region is
# content-anchored: rows committed to scrollback push the box down, rows the
# strip gives up pull it back. A cell that stood taller live than the rows it
# committed therefore lifted the box the moment it settled, and the round's
# next cell dropped it again — the reported flicker:
#
#   thinking → flicker → the tool's cell → thinking → flicker → bash → flicker → bash
#
# - the `● Thinking…` block stood up to four rows over a settled line of one;
#   the phase ends at the call's first piece and the call has a cell only
#   once its arguments have streamed, so the box sat up to three rows high
#   for as long as the model took to write the call;
# - a running command tailed four rows over its clock row (and, three
#   seconds in, a Ctrl+B hint row) and settled into three rows over the hint
#   — or into ONE, when a blank line closed the peek early;
# - the collapsed read group stood a header over a hint row and settled into
#   one line, with the rest of its batch still up under it.
#
# Every live cell now stands on the rows it settles into, so nothing is given
# back: the thought is one row, the running command takes the settled fold's
# own rows, and the hints ride rows the cells already have.
#
# Each turn is sampled ~20 times a second for as long as its status line is
# up, and must show:
#   - NO HOP: the footer's row never moves up between two samples (confirmed
#     against a second capture a beat later — Phase 121's rule: `capture-pane`
#     can read a synchronized frame half-applied, which lasts microseconds
#     where a hop lasts as long as the next cell takes to arrive);
#   - NO HELD ROWS: never more than one blank row directly above the status
#     line. Padding the strip with blank rows hides a hop too, and leaves
#     three empty lines between a settled cell and the status line for as
#     long as the next request takes — the first fix this replaced;
#   - ONE-ROW THOUGHTS: the row under a live `● Thinking…` is the strip's
#     gap, never a `⎿` body.
# Five turns:
#   1. the default dummy turn with Verbose output off — a paragraph, a
#      thought, then a Read+Edit+Bash batch written over about a second: the
#      thought → call hand-off and the read group closing under its batch;
#   2. the same turn on a short terminal, the box on the bottom row;
#   3. the dummy's parallel batch — three commands tailing and settling;
#   4. the REAL backend against a paced stub provider, one turn through every
#      hand-off in the report: thought → a `bash` call written over about a
#      second → a tall tail settling → a round boundary → thought → a command
#      whose output opens on a title and a blank line (the three-row hop) →
#      thought → a `taskcreate`, which has no cell at all → thought → the
#      reply's first words;
#   5. that turn's first command again, slow enough for the Ctrl+B hint to
#      fall due before it ends.
S153="${S}_livecells"
S153S="${S}_livecellsshort"
S153P="${S}_livecellspar"
S153R="${S}_livecellsreal"
S153H="${S}_livecellshint"
RAW153="$SMOKE_TMP/phase153.raw"

# frame_facts CAPTURE FOOTER — one pass over a capture, printing seven fields:
#   status  1 when the status line is up
#   thought 1 when a thought shows beside the live `Thinking…` label
#   tail    1 when a running command's clock row shows
#   hint    1 when the Ctrl+B hint shows on a row that carries something else
#   row     the row the FOOTER sits on (its last match), 0 when it is not there
#   gap     the blank rows directly above the status line, -1 with none up
#   tall    1 when the row under the `Thinking…` label (its bullet shown or
#           blinked away) holds anything at all: a block taller than one row
# One awk instead of a grep per question: a turn is sampled twenty times a
# second for most of a minute, and a dozen forks a sample is load the phases
# running beside this one would feel.
frame_facts() {
	printf '%s\n' "$1" | awk -v footer="$2" '
		BEGIN { status = thought = tail = hint = tall = row = blank = under = 0; gap = -1 }
		{
			if (under) {
				if ($0 !~ /^[[:space:]]*$/) tall = 1
				under = 0
			}
			if ($0 ~ /^(● |  )Thinking…/) {
				under = 1
				if ($0 ~ /^(● |  )Thinking… [^[:space:]]/) thought = 1
			}
			if ($0 ~ /^ +\+[0-9]+ lines \([0-9]+s · wait /) tail = 1
			if ($0 ~ /[^[:space:]] \(ctrl\+b to run in background\)/) hint = 1
			if (index($0, footer) > 0) row = NR
			if (!status && index($0, "esc to interrupt") > 0) {
				status = 1
				gap = blank
			}
			if ($0 ~ /^[[:space:]]*$/) blank++
			else blank = 0
		}
		END { print status, thought, tail, hint, row, gap, tall }'
}

# watch_turn SESSION FOOTER — sample a turn until its status line goes; sets
# W_SAMPLES, W_HOPS (confirmed rises of the FOOTER row while the status line
# was up), W_GAPS (confirmed samples with more than one blank row above the
# status line), W_TALL (confirmed samples of a thinking block taller than one
# row), and the liveness marks W_THOUGHT (a sample showed a thought beside
# the label), W_TAIL (a running command's clock row) and W_HINT (the Ctrl+B
# hint, on a row that carries something else too).
watch_turn() {
	local sess="$1" footer="$2" vis again prev=0 seen=0
	local status thought tail hint now gap tall
	local status2 now2 gap2 tall2 _f
	W_SAMPLES=0
	W_HOPS=0
	W_GAPS=0
	W_TALL=0
	W_THOUGHT=0
	W_TAIL=0
	W_HINT=0
	for _ in $(seq 1 900); do # up to ~45s
		vis="$(tmux capture-pane -t "$sess" -p)"
		W_SAMPLES=$((W_SAMPLES + 1))
		read -r status thought tail hint now gap tall <<<"$(frame_facts "$vis" "$footer")"
		if [ "${status:-0}" -eq 1 ]; then
			seen=1
		elif [ "$seen" -eq 1 ]; then
			# The turn is over: its end collapses the strip into the summary,
			# which is a settle rather than a hop.
			break
		fi
		[ "${thought:-0}" -eq 1 ] && W_THOUGHT=1
		[ "${tail:-0}" -eq 1 ] && W_TAIL=1
		[ "${hint:-0}" -eq 1 ] && W_HINT=1
		# Each verdict is confirmed against a second capture a beat later
		# (Phase 121's rule): `capture-pane` can read a synchronized frame
		# half-applied, which lasts microseconds where a hop, a held gap or a
		# tall block lasts as long as the next cell takes to arrive.
		if [ "${now:-0}" -gt 0 ] && [ "$now" -lt "$prev" ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			read -r status2 _f _f _f now2 _f _f <<<"$(frame_facts "$again" "$footer")"
			if [ "${status2:-0}" -eq 1 ] && [ "${now2:-0}" -gt 0 ] && [ "$now2" -lt "$prev" ]; then
				W_HOPS=$((W_HOPS + 1))
				if [ "$W_HOPS" -le 3 ]; then
					dump "$sess: the box hopped up from row $prev to row $now2 (sample $W_SAMPLES)" "$again"
				fi
				now="$now2"
			elif [ "${now2:-0}" -gt 0 ]; then
				now="$now2"
			fi
		fi
		if [ "${now:-0}" -gt 0 ]; then
			prev="$now"
		fi
		if [ "${gap:--1}" -gt 1 ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			read -r _f _f _f _f _f gap2 _f <<<"$(frame_facts "$again" "$footer")"
			if [ "${gap2:--1}" -gt 1 ]; then
				W_GAPS=$((W_GAPS + 1))
				if [ "$W_GAPS" -le 2 ]; then
					dump "$sess: $gap2 blank rows above the status line (sample $W_SAMPLES)" "$again"
				fi
			fi
		fi
		if [ "${tall:-0}" -eq 1 ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			read -r _f _f _f _f _f _f tall2 <<<"$(frame_facts "$again" "$footer")"
			if [ "${tall2:-0}" -eq 1 ]; then
				W_TALL=$((W_TALL + 1))
				if [ "$W_TALL" -le 2 ]; then
					dump "$sess: the live thinking block is taller than one row (sample $W_SAMPLES)" "$again"
				fi
			fi
		fi
		sleep 0.04
	done
	[ "$seen" -eq 1 ] || fail "$sess: the turn never showed its status line"
	if [ "$W_SAMPLES" -lt 20 ]; then
		fail "$sess: only $W_SAMPLES samples of the turn: the watch saw too little to mean anything"
	fi
}

# expect_still WHAT — the verdicts on the last watch_turn.
expect_still() {
	note "$1: sampled $W_SAMPLES times; hops=$W_HOPS; held gaps=$W_GAPS; tall thoughts=$W_TALL; saw a thought=$W_THOUGHT, a tail=$W_TAIL, the hint=$W_HINT"
	if [ "$W_HOPS" -ne 0 ]; then
		fail "$1: the box hopped up $W_HOPS time(s) mid-turn: a live cell stood taller than the rows it settled into (see the frames above)"
	fi
	if [ "$W_GAPS" -ne 0 ]; then
		fail "$1: $W_GAPS sample(s) held more than one blank row above the status line: rows were padded blank instead of the cell settling in place (see the frames above)"
	fi
	if [ "$W_TALL" -ne 0 ]; then
		fail "$1: $W_TALL sample(s) showed a live thinking block taller than the one row it settles into (see the frames above)"
	fi
}

# ---- 1. the dummy turn, Verbose output off, with room below the box ----------
APP153="env $CFG_ENV_NOHIST ALTER_ZERO_VERBOSE=0 ALTER_ZERO_STARTUP_DELAY_MS=$SMOKE_STARTUP_MS $BIN"
launch "$S153" 80 24 "$APP153"
tmux pipe-pane -t "$S153" -o "cat > $RAW153"
submit "$S153" "$USER_MSG"
watch_turn "$S153" 'dummy_model_name ·'
expect_still "the default turn"
[ "$W_THOUGHT" -eq 1 ] || fail "the default turn: no sample showed the thought beside its '● Thinking…' label, so the hand-off was not watched"
wait_summaries 20 "$S153" 1 >/dev/null || fail "the default turn never finished"
tmux pipe-pane -t "$S153" # close the recording before quitting

final="$(pane "$S153" -S -200)"
dump "captured pane + scrollback (the settled turn)" "$final"
# Liveness: the turn went through both hand-offs, so a quiet watch is a real pass.
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$final")" "1" \
	"the thought did not settle into exactly one 'Thought for' line"
expect_lacks "$final" -F "Then edit it and run it." "the thought's own text reached the conversation: only its line may commit"
expect_eq "$(grep -cE '^Read 1 file \(ctrl\+o to expand\)' <<<"$final")" "1" "the read group's line is not there exactly once"
expect_eq "$(grep -cF '● Edit(about.py)' <<<"$final")" "1" "the edit's cell is not there exactly once"
expect_eq "$(grep -cF '● Bash(python3 about.py)' <<<"$final")" "1" "the run's cell is not there exactly once"
purges=$(grep -c $'\x1b\\[3J' "$RAW153" 2>/dev/null || true)
expect_eq "${purges:-0}" "0" "the turn purge-rebuilt the screen"
keys "$S153" C-c
wait_gone 10 "$S153" || fail "the app did not quit on Ctrl+C"

# ---- 2. the same turn on a short terminal: the box on the bottom row ---------
launch "$S153S" 80 16 "$APP153"
submit "$S153S" "$USER_MSG"
watch_turn "$S153S" 'dummy_model_name ·'
expect_still "the short terminal"
short="$(wait_summaries 20 "$S153S" 1)" || fail "the short session's turn never finished"
footer_row="$(printf '%s\n' "$short" | grep -nF 'dummy_model_name ·' | tail -1 | cut -d: -f1)"
expect_eq "${footer_row:-0}" "16" "the settled box is not on the bottom row"
expect_eq "$(count_footers "$short")" "1" "the settled short session shows the footer more than once"
keys "$S153S" C-c
wait_gone 10 "$S153S" || fail "the short session did not quit on Ctrl+C"

# ---- 3. three commands in one batch, each tailing and then settling ----------
launch "$S153P" 80 24
submit "$S153P" "run it in parallel"
watch_turn "$S153P" 'dummy_model_name ·'
expect_still "the parallel batch"
[ "$W_TAIL" -eq 1 ] || fail "the parallel batch: no sample showed a running command's clock row, so no tail was watched settling"
par="$(wait_summaries 20 "$S153P" 1 -S -200)" || fail "the parallel turn never finished"
expect_eq "$(grep -cF '● Bash(ping -c 20' <<<"$par")" "3" "the batch's three cells are not all there exactly once"
keys "$S153P" C-c
wait_gone 10 "$S153P" || fail "the parallel session did not quit on Ctrl+C"

# ---- the real backend against a paced stub provider -------------------------
# The dummy scripts its events; these turns go through `LlmBackend` and
# `stream_round` the way a provider's stream does: reasoning deltas, then a
# tool call written a few characters at a time (the phase ends at its first
# piece; its cell exists only once the last is in), then the stream's end.
# The stub picks its script by the user's message and its round by how many
# assistant messages follow it.
SR_PORT="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 - "$SR_PORT" <<'PY' &
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port = int(sys.argv[1])

THOUGHT = [
    "Let me run the command first and see what it prints before answering.\n",
    "It prints thirty lines, a few milliseconds apart, so the tail will move.\n",
    "Then I can read the output and say what it was.\n",
    "After that there is nothing left to do but report back.\n",
]
LONG = "for i in $(seq 1 30); do echo line-$i; sleep 0.05; done"
BLANKY = "echo title; echo; for i in $(seq 1 12); do echo row-$i; sleep 0.1; done"
SLOW = "for i in $(seq 1 9); do echo tick-$i; sleep 0.6; done"

def said(messages):
    """Every user-role text of the request, joined."""
    out = []
    for message in messages:
        if message.get("role") != "user":
            continue
        content = message.get("content")
        if isinstance(content, str):
            out.append(content)
        elif isinstance(content, list):
            out.extend(part.get("text", "") for part in content if isinstance(part, dict))
    return "\n".join(out)

def rounds_done(messages):
    """How many assistant rounds follow the user's message."""
    n = 0
    for message in reversed(messages):
        if message.get("role") == "user":
            break
        if message.get("role") == "assistant":
            n += 1
    return n

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

    def think(self):
        for line in THOUGHT:
            words = line.split(" ")
            for i, word in enumerate(words):
                self.emit({"reasoning_content": word + ("" if i == len(words) - 1 else " ")}, 0.025)

    def call(self, name, args, pause):
        # The thought settles on the first piece; the call is announced only
        # once the last one is in.
        pieces = [args[i:i + 12] for i in range(0, len(args), 12)]
        self.emit({"tool_calls": [{"index": 0, "id": "call_153", "type": "function",
                                   "function": {"name": name, "arguments": pieces[0]}}]}, pause)
        for piece in pieces[1:]:
            self.emit({"tool_calls": [{"index": 0, "function": {"arguments": piece}}]}, pause)
        self.emit({}, finish="tool_calls")

    def bash(self, command):
        self.call("bash", json.dumps({"command": command, "description": "Run it"}), 0.1)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length") or 0)))
        messages = body.get("messages", [])
        script = "hint" if "wait for the hint" in said(messages) else "walk"
        n = rounds_done(messages)
        # HTTP/1.0 with no length: the stream ends when the connection does,
        # so each frame reaches the client as it is written.
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        if n > 0:
            time.sleep(1.2)  # the round boundary: the next request's time to first token
        if script == "hint":
            if n == 0:
                self.bash(SLOW)
            else:
                self.emit({"content": "Done."})
                self.emit({}, finish="stop")
        elif n == 0:
            self.think()
            self.bash(LONG)
        elif n == 1:
            self.think()
            self.bash(BLANKY)
        elif n == 2:
            self.think()
            self.call("taskcreate", json.dumps({"subject": "Count the lines",
                                                "description": "Count them"}), 0.0)
        else:
            self.think()
            for word in "Both ran: the first ended on line-30 and the second on row-12.".split(" "):
                self.emit({"content": word + " "}, 0.05)
            self.emit({}, finish="stop")
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def log_message(self, *_args):
        pass

ThreadingHTTPServer(("127.0.0.1", port), Stub).serve_forever()
PY
SR_STUB=$!
smoke_on_exit kill "$SR_STUB"
for _ in $(seq 1 50); do # wait for the stub to listen
	if python3 -c "import socket, sys; s = socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(('127.0.0.1', $SR_PORT)) == 0 else 1)" 2>/dev/null; then
		break
	fi
	sleep 0.1
done
SR_PROVIDERS="$SMOKE_TMP/providers153.toml"
printf '[providers.stub]\nname = "Stub"\n\n[providers.stub.kwargs]\napi_base = "http://127.0.0.1:%s/v1"\n' "$SR_PORT" >"$SR_PROVIDERS"
# Master mode: the commands run unasked, so nothing but the strip is on screen.
SR_APP="env NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 $CFG_ENV_NOHIST ALTER_ZERO_PERMISSION_MODE=master ALTER_ZERO_PROVIDERS_FILE=$SR_PROVIDERS ALTER_ZERO_PROVIDER=stub ALTER_ZERO_MODEL=stub-model STUB_API_KEY=stand-in $BIN_ABS"

# ---- 4. every hand-off in the report, one after another ---------------------
launch -c "$(work_dir real)" "$S153R" 100 30 "$SR_APP"
submit "$S153R" "walk the hand-offs"
watch_turn "$S153R" 'stub-model ·'
expect_still "the real backend"
[ "$W_THOUGHT" -eq 1 ] || fail "the real backend: no sample showed a thought beside its label"
[ "$W_TAIL" -eq 1 ] || fail "the real backend: no sample showed a running command's clock row"
real="$(wait_settled 10 "$S153R" -S -80 -- -E "^$SUMMARY_RE")" || fail "the stub's turn never settled"
dump "the settled real-backend session" "$real"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$real")" "4" \
	"the stub's four thoughts did not each settle into one 'Thought for' line"
expect_eq "$(grep -cF '● Bash(for i in' <<<"$real")" "1" "the stub's first command is not there exactly once"
expect_eq "$(grep -cF '● Bash(echo title' <<<"$real")" "1" "the stub's second command is not there exactly once"
# The second command settles into its first block over the hint — the cell
# its tail had to be no taller than.
expect_has "$real" -E '^  ⎿  title$' "the second command's peek is not its first block"
expect_has "$real" -F "Both ran: the first ended on line-30" "the stub's reply never arrived"
expect_lacks "$real" -F "Let me run the command first" "a thought's own text reached the conversation: only its line may commit"
keys "$S153R" C-c
wait_gone 10 "$S153R" || fail "the real-backend session did not quit on Ctrl+C"

# ---- 5. a command slow enough for the Ctrl+B hint to fall due ----------------
# The hint used to be a row of its own under the cell, three seconds into a
# command: a row the settled cell has no row for.
launch -c "$(work_dir hint)" "$S153H" 100 30 "$SR_APP"
submit "$S153H" "wait for the hint"
watch_turn "$S153H" 'stub-model ·'
expect_still "the Ctrl+B hint"
[ "$W_HINT" -eq 1 ] || fail "the Ctrl+B hint: no sample showed it riding a row of the running cell, so its settle was not watched"
hinted="$(wait_settled 10 "$S153H" -S -80 -- -E "^$SUMMARY_RE")" || fail "the slow command's turn never settled"
expect_eq "$(grep -cF '● Bash(for i in $(seq 1 9)' <<<"$hinted")" "1" "the slow command's cell is not there exactly once"
expect_lacks "$hinted" -F "ctrl+b" "the Ctrl+B hint reached the conversation: it is live-only"
keys "$S153H" C-c
wait_gone 10 "$S153H" || fail "the hint session did not quit on Ctrl+C"
