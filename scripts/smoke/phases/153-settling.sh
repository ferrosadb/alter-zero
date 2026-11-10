#!/usr/bin/env bash
# Phase 153 — a finished cell keeps its rows until the next one takes them: the box never jumps up mid-turn, and no row is padding

. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
smoke_begin

# SETTLING CELLS (docs/settling.md). A live cell is taller than what it
# commits — a `● Thinking…` block's three rows of thought over its one
# `Thought for …` line, a running command's four-row tail and clock over its
# three-row peek — and the live region is anchored at its top. So a cell that
# collapsed the moment it ended pulled the box UP by the difference, and the
# next cell pushed it back down: the flicker reported between a thought and
# the tool call it leads to, and between one command and the next.
#
# A cell that ends now KEEPS THE ROWS IT STOOD ON and gives them up one for
# one as whatever comes next needs them — its committed lines reaching
# scrollback the frame it is down to them. A thought keeps its `● Thinking…`
# block exactly as it was; a command shows that it finished, its bullet in
# the outcome's colour and its clock stopped. Three things this phase holds,
# on every sample of every turn:
#
#   1. the box never moves up while the turn is in flight;
#   2. the status line has exactly ONE blank row above it — a finished cell's
#      rows are never held as blank padding (what the first fix for this did,
#      and was reported for: "it occupies 3 empty newlines");
#   3. a thought's settled `Thought for …` line is never on screen over the
#      thought it stands for (what the second fix did: it read as a flash of
#      stale thought). The line appears alone, the frame the next cell lands.
#
# Each turn is sampled ~15 times a second, and a candidate is confirmed
# against a second capture a beat later — Phase 121's rule: a torn frame lasts
# microseconds, a hop or a held gap as long as the next cell takes to arrive.
# Four turns:
#   1. the default dummy turn — a paragraph, a two-line thought, then a
#      Read+Edit+Bash batch written over about a second — with room below the
#      box, its byte stream checked for purge rebuilds and its cells for
#      landing exactly once;
#   2. the same turn on a short terminal, where the box sits on the bottom row
#      and a rise strands blank rows under it;
#   3. the REAL backend against a paced stub provider: a thought, a command
#      whose call is written over about a second (the thought's block standing
#      unchanged meanwhile), a three-second wait for the next round (the
#      finished command standing on its rows, clock stopped), a second command
#      whose settled peek is cut at a blank line — three rows shorter than the
#      cell that ran — and a thought before the answer;
#   4. a thought whose round records only a task call — no cell ever lands to
#      take its rows — and a next request that fails once: the thought's line
#      still reaches scrollback, exactly once.
S153="${S}_settling"
S153S="${S}_settlingshort"
RAW153="$SMOKE_TMP/phase153.raw"
THOUGHT153="Let me read the file first."

# The blank rows directly above the status line in a capture, or nothing when
# no status line is on screen.
gap_above_status() {
	printf '%s\n' "$1" | awk '
		/esc to interrupt/ { print blank; exit }
		/^[[:space:]]*$/ { blank++; next }
		{ blank = 0 }'
}

# The rows of a capture on which a settled `Thought for …` line sits directly
# over a `⎿` row: the line drawn over its own thought. A committed line is
# followed by its spacer, and the strip draws the line only once the thought's
# rows are gone, so there should never be one.
stale_thoughts() {
	printf '%s\n' "$1" | awk '
		prev ~ /^Thought for [0-9]/ && /^  ⎿  / { n++ }
		{ prev = $0 }
		END { print n + 0 }'
}

# watch_turn SESSION FOOTER — sample a turn until its status line goes; sets
# W_SAMPLES, W_HOPS (confirmed rises of the FOOTER row while the status line
# was up), W_GAPS (confirmed samples with more than one blank row above the
# status line), W_STALE (confirmed samples showing a settled `Thought for`
# line directly over a row of its thought) and W_COMMAND (a sample showed a
# command's clock row stopped — `+26 lines (1s)`, no wait left to count — the
# finished command standing on the rows it ran on).
watch_turn() {
	local sess="$1" footer="$2" vis again now now_again prev=0 seen=0 gap
	W_SAMPLES=0
	W_HOPS=0
	W_GAPS=0
	W_STALE=0
	W_COMMAND=0
	for _ in $(seq 1 900); do # up to ~45s
		vis="$(tmux capture-pane -t "$sess" -p)"
		W_SAMPLES=$((W_SAMPLES + 1))
		if printf '%s' "$vis" | grep -qF "esc to interrupt"; then
			seen=1
		elif [ "$seen" -eq 1 ]; then
			# The turn is over: its end collapses the strip into the summary,
			# which is a settle rather than a hop.
			break
		fi
		if [ "$(stale_thoughts "$vis")" -gt 0 ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			if [ "$(stale_thoughts "$again")" -gt 0 ]; then
				W_STALE=$((W_STALE + 1))
				if [ "$W_STALE" -le 2 ]; then
					dump "$sess: a settled thought's line over its own thought (sample $W_SAMPLES)" "$again"
				fi
			fi
		fi
		if printf '%s\n' "$vis" | grep -qE '^ +\+[0-9]+ lines \([0-9]+s\)$'; then
			W_COMMAND=1
		fi
		now=$(printf '%s\n' "$vis" | grep -nF "$footer" | tail -1 | cut -d: -f1)
		if [ "${now:-0}" -gt 0 ] && [ "$now" -lt "$prev" ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			now_again=$(printf '%s\n' "$again" | grep -nF "$footer" | tail -1 | cut -d: -f1)
			if printf '%s' "$again" | grep -qF "esc to interrupt" &&
				[ "${now_again:-0}" -gt 0 ] && [ "$now_again" -lt "$prev" ]; then
				W_HOPS=$((W_HOPS + 1))
				if [ "$W_HOPS" -le 2 ]; then
					dump "$sess: the box jumped up from row $prev to row $now_again (sample $W_SAMPLES)" "$again"
				fi
			else
				now="${now_again:-$now}"
			fi
		fi
		prev="${now:-$prev}"
		gap="$(gap_above_status "$vis")"
		if [ "${gap:-1}" -gt 1 ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			gap="$(gap_above_status "$again")"
			if [ "${gap:-1}" -gt 1 ]; then
				W_GAPS=$((W_GAPS + 1))
				if [ "$W_GAPS" -le 2 ]; then
					dump "$sess: $gap blank rows above the status line (sample $W_SAMPLES)" "$again"
				fi
			fi
		fi
		sleep 0.05
	done
	[ "$seen" -eq 1 ] || fail "$sess: the turn never showed its status line"
	if [ "$W_SAMPLES" -lt 20 ]; then
		fail "$sess: only $W_SAMPLES samples of the turn: the watch saw too little to mean anything"
	fi
}

# expect_watch WHAT — the verdicts on the last watch_turn.
expect_watch() {
	if [ "$W_STALE" -ne 0 ]; then
		fail "$1: $W_STALE sample(s) showed a settled thought's line over its own thought: the line may only be drawn alone, once the thought's rows are gone (see the frames above)"
	fi
	if [ "$W_HOPS" -ne 0 ]; then
		fail "$1: the box jumped up $W_HOPS time(s) mid-turn: a finished cell gave its rows up before anything had taken them (see the frames above)"
	fi
	if [ "$W_GAPS" -ne 0 ]; then
		fail "$1: $W_GAPS sample(s) held more than one blank row above the status line: rows were held as padding (see the frames above)"
	fi
}

# ---- 1. the dummy turn, with room below the box ------------------------------
launch "$S153" 80 24
tmux pipe-pane -t "$S153" -o "cat > $RAW153"
submit "$S153" "$USER_MSG"
watch_turn "$S153" 'dummy_model_name ·'
note "tall: sampled $W_SAMPLES times; jumps=$W_HOPS; padded samples=$W_GAPS; lines over their thought=$W_STALE"
expect_watch "tall"
wait_summaries 20 "$S153" 1 >/dev/null || fail "the default turn never finished"
tmux pipe-pane -t "$S153" # close the recording before quitting

final="$(pane "$S153" -S -200)"
dump "captured pane + scrollback (the settled turn)" "$final"
# Liveness, and the other half of the bargain: a cell that waited in the strip
# reaches scrollback once, in order, and the thought's text never does.
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$final")" "1" \
	"the thought did not settle into exactly one 'Thought for' line"
expect_lacks "$final" -F "$THOUGHT153" "the thought's own text reached the conversation: only its line may commit"
expect_eq "$(grep -cF '● Read(about.py)' <<<"$final")" "1" "the read's cell is not there exactly once"
expect_eq "$(grep -cF '● Edit(about.py)' <<<"$final")" "1" "the edit's cell is not there exactly once"
expect_eq "$(grep -cF '● Bash(python3 about.py)' <<<"$final")" "1" "the run's cell is not there exactly once"
order="$(grep -oE '^Thought for |^● (Read|Edit)\(about\.py\)|^● Bash\(python3 about\.py\)' <<<"$final" |
	sed -E 's/^Thought for $/Thought/; s/^.* ([A-Za-z]+)\(.*$/\1/' | tr '\n' ' ')"
expect_eq "$order" "Thought Read Edit Bash " "the cells reached scrollback out of order"
purges=$(grep -c $'\x1b\\[3J' "$RAW153" 2>/dev/null || true)
expect_eq "${purges:-0}" "0" "the turn purge-rebuilt the screen"
keys "$S153" C-c
wait_gone 10 "$S153" || fail "the app did not quit on Ctrl+C"

# ---- 2. the dummy turn on a short terminal: the box on the bottom row --------
launch "$S153S" 80 16
submit "$S153S" "$USER_MSG"
watch_turn "$S153S" 'dummy_model_name ·'
note "short: sampled $W_SAMPLES times; jumps=$W_HOPS; padded samples=$W_GAPS; lines over their thought=$W_STALE"
expect_watch "short"
short="$(wait_summaries 20 "$S153S" 1)" || fail "the short session's turn never finished"
# Settled, the box is flush at the bottom: nothing kept outlived the turn.
footer_row="$(printf '%s\n' "$short" | grep -nF 'dummy_model_name ·' | tail -1 | cut -d: -f1)"
expect_eq "${footer_row:-0}" "16" "the settled box is not on the bottom row: a kept row outlived the turn"
expect_eq "$(count_footers "$short")" "1" "the settled short session shows the footer more than once"
keys "$S153S" C-c
wait_gone 10 "$S153S" || fail "the short session did not quit on Ctrl+C"

# ---- the real backend against a paced stub provider ------------------------
# The dummy scripts its events; these two turns go through `LlmBackend` and
# `stream_round` the way a provider's stream does. The stub picks its script
# by the user's message and its round by the tool results the request holds:
#   "run the commands" —
#     round 1: a three-line thought, then a `bash` call whose arguments
#       stream over about a second (the window the thought's block stands
#       in); the command prints thirty lines, so its running cell stands a
#       row taller than its settled peek;
#     round 2: three seconds of nothing — the settled command standing on its
#       rows — then a second `bash` call whose output is one line, a blank
#       line, six more: its settled peek stops at the blank, three rows
#       shorter than the cell that ran;
#     round 3: a wait, a two-line thought, the answer.
#   "plan the work" — a thought, then only a `taskcreate`, which records no
#     cell, and a next request that fails with a 500 once before it answers.
SR_PORT="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 - "$SR_PORT" <<'PY' &
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port = int(sys.argv[1])
failed = set()  # the scripts whose second round has failed once already

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

    def think(self, lines):
        for line in lines:
            self.emit({"reasoning_content": line}, 0.15)

    def call(self, name, args, pause):
        # The thought settles on the first piece; the call has a cell only
        # once the last one is in and its batch is announced.
        pieces = [args[i:i + 12] for i in range(0, len(args), 12)]
        self.emit({"tool_calls": [{"index": 0, "id": "call_153", "type": "function",
                                   "function": {"name": name, "arguments": pieces[0]}}]}, pause)
        for piece in pieces[1:]:
            self.emit({"tool_calls": [{"index": 0, "function": {"arguments": piece}}]}, pause)
        self.emit({}, finish="tool_calls")

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length") or 0)))
        messages = body.get("messages", [])
        script = "plan" if "plan the work" in said(messages) else "run"
        # The round: how many tool results the conversation holds so far.
        done = sum(1 for message in messages if message.get("role") == "tool")
        if script == "plan" and done == 1 and script not in failed:
            failed.add(script)
            data = b'{"error":{"message":"stub overloaded","type":"server_error"}}'
            self.send_response(500)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        # HTTP/1.0 with no length: the stream ends when the connection does,
        # so each frame reaches the client as it is written.
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        if script == "plan":
            if done == 0:
                self.think(["Plan the work before doing it.\n", "One task covers it.\n"])
                self.call("taskcreate", json.dumps({"subject": "Count the lines",
                                                    "description": "Count them"}), 0.0)
            else:
                self.emit({"content": "Done."})
                self.emit({}, finish="stop")
        elif done == 0:
            self.think(["Let me run the command first.\n", "It prints thirty lines.\n",
                        "Then I can answer.\n"])
            command = "for i in $(seq 1 30); do echo line-$i; sleep 0.05; done"
            self.call("bash", json.dumps({"command": command, "description": "Print thirty lines"}), 0.12)
        elif done == 1:
            time.sleep(3)  # the wait between rounds: the first command stands settled
            command = "echo first; echo; for i in 1 2 3 4 5 6; do echo more-$i; sleep 0.05; done"
            self.call("bash", json.dumps({"command": command, "description": "Print a gap"}), 0.02)
        else:
            time.sleep(1.5)  # …and the second, three rows taller than its peek
            self.think(["Both commands ran.\n", "Answer in one word.\n"])
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
# Master mode: the stub's own two commands run unasked, so nothing but the
# strip is on screen while they do.
SR_APP="env NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 $CFG_ENV_NOHIST ALTER_ZERO_PERMISSION_MODE=master ALTER_ZERO_PROVIDERS_FILE=$SR_PROVIDERS ALTER_ZERO_PROVIDER=stub ALTER_ZERO_MODEL=stub-model STUB_API_KEY=stand-in $BIN_ABS"

# ---- 3. "run the commands": every hand-off a round makes ---------------------
S153R="${S}_settlingreal"
launch -c "$(work_dir real)" "$S153R" 100 30 "$SR_APP"
submit "$S153R" "run the commands"
watch_turn "$S153R" 'stub-model ·'
note "real backend: sampled $W_SAMPLES times; jumps=$W_HOPS; padded samples=$W_GAPS; lines over their thought=$W_STALE; finished command seen standing=$W_COMMAND"
expect_watch "real backend"
if [ "$W_COMMAND" -ne 1 ]; then
	fail "real backend: the finished command never kept its rows: no sample showed its clock stopped between rounds"
fi
real="$(wait_pane 20 "$S153R" -S -80 -- -F "Done.")" || fail "the stub's last round never answered"
real="$(wait_summaries 10 "$S153R" 1 -S -80)" || fail "the stub's turn never finished"
dump "the settled real-backend session" "$real"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$real")" "2" \
	"the stub's two thoughts did not settle into exactly two 'Thought for' lines"
expect_eq "$(grep -cF '● Bash(for i in $(seq 1 30)' <<<"$real")" "1" "the first command's cell is not there exactly once"
expect_eq "$(grep -cF '● Bash(echo first; echo;' <<<"$real")" "1" "the second command's cell is not there exactly once"
expect_lacks "$real" -F "Let me run the command first." "the stub's thought reached the conversation: only its line may commit"
# What reached scrollback is the committed form — the peek from the top and
# its hint — never the tail the settling cell was showing, nor its clock.
expect_has "$real" -E '^ +… \+27 lines \(ctrl\+o to expand\)$' "the first command did not commit its head peek"
expect_lacks "$real" -E '^ +\+[0-9]+ lines \([0-9]+s\)$' "a settling cell's stopped clock row reached the conversation"
expect_lacks "$real" -F "line-30" "the first command's tail reached the conversation: its peek is the head"
keys "$S153R" C-c
wait_gone 10 "$S153R" || fail "the real-backend session did not quit on Ctrl+C"

# ---- 4. "plan the work": a thought, a task call with no cell, a retry -------
S153T="${S}_settlingtask"
launch -c "$(work_dir task)" "$S153T" 100 30 "$SR_APP"
submit "$S153T" "plan the work"
wait_pane 25 "$S153T" -S -60 -- -F "Done." >/dev/null || fail "the retried second round never answered"
task="$(wait_settled 5 "$S153T" -S -60 -- -E "^$SUMMARY_RE")"
dump "the settled task-call session" "$task"
expect_has "$task" -F "Count the lines" "the task call never reached the checklist"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$task")" "1" \
	"the thought before the task call is not there exactly once: its line waited in the strip and never reached scrollback, or reached it twice"
expect_lacks "$task" -F "Plan the work before doing it." "the thought's own text reached the conversation: only its line may commit"
keys "$S153T" C-c
wait_gone 10 "$S153T" || fail "the task-call session did not quit on Ctrl+C"
