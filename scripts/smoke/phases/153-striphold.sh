#!/usr/bin/env bash
# Phase 153 — a hand-off between live cells never lifts the box, and a quick thought puts no block up
# smoke: tags=serial

. "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
smoke_begin

# THE STRIP HOLD (docs/strip-hold.md). The live region is content-anchored:
# a frame that commits `c` rows pushes the box down by `c`, a region `h` rows
# shorter pulls it up by `h`. Every hand-off between two live cells shed more
# rows than it committed — a settled thought's block before the round's call
# had a cell, a command's clock row as its tail settled — so the box rose and
# dropped back a fraction of a second later: the reported flicker.
#
# Two rules answer it. A thought shorter than REASONING_SHOW_DELAY (1 s) puts
# no `● Thinking…` block up at all — it would collapse before anyone could
# read it. And the strip holds the rows a hand-off gave back as blank rows
# over the status line until content takes them, commits cover them, or
# STRIP_HOLD (1.5 s) passes — never longer, which is what the first fix got
# wrong: its rows lingered as empty lines for as long as a slow request took.
#
# Each turn is sampled ~20 times a second. A rise of the footer while the
# status line is up counts only when a second capture a beat later confirms
# it (Phase 121's rule: a torn capture lasts microseconds, a hop as long as
# the next cell takes). A run of more than one blank row directly above the
# status line may last as long as a hold can — STRIP_HOLD, plus a hidden
# thought it waits on — and a run past HOLD_LIMIT_MS is a hold that never let
# go. Turns:
#   1. the default dummy turn — a two-second thought, then a Read+Edit+Bash
#      batch generated over about a second — with room below the box;
#   2. the same on a short terminal, where the box sits on the bottom row;
#   3–5. the REAL backend against a paced stub provider: quick thoughts
#      handing over to a `bash` call whose tail settles, round gaps well
#      inside the hold; a quick thought handing over to a `skill` call; and a
#      command's tail settling before a next round slower than the hold, whose
#      held row must lapse rather than linger.
S153="${S}_striphold"
S153S="${S}_stripholdshort"
RAW153="$SMOKE_TMP/phase153.raw"
HOLD_LIMIT_MS=2800

# The blank rows directly above the status line in a capture, or nothing when
# no status line is on screen.
gap_above_status() {
	printf '%s\n' "$1" | awk '
		/esc to interrupt/ { print blank; exit }
		/^[[:space:]]*$/ { blank++; next }
		{ blank = 0 }'
}

now_ms() { date +%s%3N; }

# watch_turn SESSION FOOTER [SETTLES] — sample a turn until its status line
# goes. Sets W_SAMPLES, W_HOPS (confirmed rises of the FOOTER row while the
# status line was up, beyond SETTLES — the lapses a turn is allowed), W_HELD
# (the longest run, in ms, of more than one blank row above the status line),
# W_HELD_SEEN (any sample showed such a run) and W_BLOCK (any sample showed a
# thinking block's header).
watch_turn() {
	local sess="$1" footer="$2" settles="${3:-0}" vis again now prev=0 seen=0 gap run_from=0 t rises=0
	W_SAMPLES=0
	W_HOPS=0
	W_HELD=0
	W_HELD_SEEN=0
	W_BLOCK=0
	for _ in $(seq 1 700); do # up to ~35s
		vis="$(tmux capture-pane -t "$sess" -p)"
		t="$(now_ms)"
		W_SAMPLES=$((W_SAMPLES + 1))
		if printf '%s' "$vis" | grep -qF "esc to interrupt"; then
			seen=1
		elif [ "$seen" -eq 1 ]; then
			# The turn is over: its end collapses the strip into the summary,
			# which is a settle rather than a hop.
			break
		fi
		# The block's header, bullet blinking or not.
		if printf '%s\n' "$vis" | grep -qE '^(● |  )Thinking…'; then
			W_BLOCK=1
		fi
		now=$(printf '%s\n' "$vis" | grep -nF "$footer" | tail -1 | cut -d: -f1)
		if [ "${now:-0}" -gt 0 ] && [ "$now" -lt "$prev" ]; then
			sleep 0.03
			again="$(tmux capture-pane -t "$sess" -p)"
			local now_again
			now_again=$(printf '%s\n' "$again" | grep -nF "$footer" | tail -1 | cut -d: -f1)
			if printf '%s' "$again" | grep -qF "esc to interrupt" &&
				[ "${now_again:-0}" -gt 0 ] && [ "$now_again" -lt "$prev" ]; then
				rises=$((rises + 1))
				if [ "$rises" -gt "$settles" ]; then
					W_HOPS=$((W_HOPS + 1))
					if [ "$W_HOPS" -le 2 ]; then
						dump "$sess: the box rose from row $prev to row $now_again (sample $W_SAMPLES)" "$again"
					fi
				fi
				now="$now_again"
			else
				now="${now_again:-$now}"
			fi
		fi
		prev="${now:-$prev}"
		gap="$(gap_above_status "$vis")"
		if [ "${gap:-1}" -gt 1 ]; then
			W_HELD_SEEN=1
			[ "$run_from" -eq 0 ] && run_from="$t"
			if [ $((t - run_from)) -gt "$W_HELD" ]; then
				W_HELD=$((t - run_from))
			fi
		else
			run_from=0
		fi
		sleep 0.05
	done
	[ "$seen" -eq 1 ] || fail "$sess: the turn never showed its status line"
	if [ "$W_SAMPLES" -lt 20 ]; then
		fail "$sess: only $W_SAMPLES samples of the turn: the watch saw too little to mean anything"
	fi
}

# expect_steady WHAT — no hop, and no held row outliving a hold.
expect_steady() {
	if [ "$W_HOPS" -ne 0 ]; then
		fail "$1: the box rose $W_HOPS time(s) mid-turn: a hand-off gave rows up before the next cell could take them (see the frames above)"
	fi
	if [ "$W_HELD" -gt "$HOLD_LIMIT_MS" ]; then
		fail "$1: blank rows stood over the status line for ${W_HELD}ms: a hold that never let go"
	fi
}

# ---- 1. the dummy turn, with room below the box ------------------------------
launch "$S153" 80 30
tmux pipe-pane -t "$S153" -o "cat > $RAW153"
submit "$S153" "$USER_MSG"
watch_turn "$S153" 'dummy_model_name ·'
note "tall: sampled $W_SAMPLES times; rises=$W_HOPS; longest held run=${W_HELD}ms; block seen=$W_BLOCK"
expect_steady "tall"
# Liveness: the dummy thinks for about two seconds, so its block does show —
# from the second second on.
expect_eq "$W_BLOCK" "1" "the dummy's two-second thought never showed its block"
wait_summaries 20 "$S153" 1 >/dev/null || fail "the default turn never finished"
tmux pipe-pane -t "$S153" # close the recording before quitting
final="$(pane "$S153" -S -200)"
dump "captured pane + scrollback (the settled turn)" "$final"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$final")" "1" \
	"the thought did not settle into exactly one 'Thought for' line"
expect_eq "$(grep -cF '● Read(about.py)' <<<"$final")" "1" "the read's cell is not there exactly once"
expect_eq "$(grep -cF '● Edit(about.py)' <<<"$final")" "1" "the edit's cell is not there exactly once"
expect_eq "$(grep -cF '● Bash(python3 about.py)' <<<"$final")" "1" "the run's cell is not there exactly once"
purges=$(grep -c $'\x1b\\[3J' "$RAW153" 2>/dev/null || true)
expect_eq "${purges:-0}" "0" "the turn purge-rebuilt the screen"
keys "$S153" C-c
wait_gone 10 "$S153" || fail "the app did not quit on Ctrl+C"

# ---- 2. the dummy turn on a short terminal: the box on the bottom row --------
launch "$S153S" 80 16
submit "$S153S" "$USER_MSG"
watch_turn "$S153S" 'dummy_model_name ·'
note "short: sampled $W_SAMPLES times; rises=$W_HOPS; longest held run=${W_HELD}ms"
expect_steady "short"
short="$(wait_summaries 20 "$S153S" 1)" || fail "the short session's turn never finished"
# Settled, the box is flush at the bottom: nothing held outlived the turn.
footer_row="$(printf '%s\n' "$short" | grep -nF 'dummy_model_name ·' | tail -1 | cut -d: -f1)"
expect_eq "${footer_row:-0}" "16" "the settled box is not on the bottom row: a held row outlived the turn"
expect_eq "$(count_footers "$short")" "1" "the settled short session shows the footer more than once"
keys "$S153S" C-c
wait_gone 10 "$S153S" || fail "the short session did not quit on Ctrl+C"

# ---- the real backend against a paced stub provider ------------------------
# The dummy scripts its events; these turns go through `LlmBackend` and
# `stream_round` the way a provider's stream does. The stub picks its script
# by the user's message, and asks only for commands auto mode's read-only
# fast path runs unasked (`seq`, `sleep`, `echo`):
#   "run the chain" — a quick thought, a `bash` call streaming nine lines in
#     three bursts (its tail stands taller than its settled peek), a 0.6 s
#     round gap, a quick thought, a second call, then the answer;
#   "load the skill" — a quick thought, a `skill` call, then the answer;
#   "wait for it" — a command whose tail settles, then a next round that
#     takes four seconds, well past any hold.
SH_PORT="$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 - "$SH_PORT" <<'PY' &
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port = int(sys.argv[1])
CHAIN = "seq 1 3; sleep 0.4; seq 4 6; sleep 0.4; seq 7 9"

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
        # A quick thought: well inside the one-second show delay.
        for line in lines:
            self.emit({"reasoning_content": line}, 0.1)

    def call(self, name, args):
        # The thought settles on the first piece; the call is announced only
        # once the last one is in.
        pieces = [args[i:i + 12] for i in range(0, len(args), 12)]
        self.emit({"tool_calls": [{"index": 0, "id": "call_153", "type": "function",
                                   "function": {"name": name, "arguments": pieces[0]}}]}, 0.03)
        for piece in pieces[1:]:
            self.emit({"tool_calls": [{"index": 0, "function": {"arguments": piece}}]}, 0.03)
        self.emit({}, finish="tool_calls")

    def answer(self, words):
        for word in words:
            self.emit({"content": word}, 0.03)
        self.emit({}, finish="stop")

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length") or 0)))
        messages = body.get("messages", [])
        text = said(messages)
        rounds = sum(1 for message in messages if message.get("role") == "tool")
        # HTTP/1.0 with no length: the stream ends when the connection does,
        # so each frame reaches the client as it is written.
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        if "run the chain" in text:
            if rounds:
                time.sleep(0.6)
            if rounds == 0:
                self.think(["List the numbers first.\n", "Nine will do.\n"])
                self.call("bash", json.dumps({"command": CHAIN, "description": "Print nine numbers"}))
            elif rounds == 1:
                self.think(["Now two letters.\n"])
                self.call("bash", json.dumps({"command": "echo a; echo b", "description": "Print two letters"}))
            else:
                self.think(["Answer briefly.\n"])
                self.answer(["Both", " ran."])
        elif "load the skill" in text:
            # Long enough on either side of the hand-off for the watch to
            # sample it: a second before the thought, and after the call.
            time.sleep(1.0)
            if rounds == 0:
                self.think(["Load the search skill first.\n", "Then answer.\n"])
                self.call("skill", json.dumps({"skill": "web-search"}))
            else:
                self.think(["Loaded.\n"])
                self.answer(["Loaded", " it."])
        elif "wait for it" in text:
            if rounds == 0:
                self.call("bash", json.dumps({"command": CHAIN, "description": "Print nine numbers"}))
            else:
                time.sleep(4)  # well past any hold
                self.answer(["Waited."])
        else:
            self.answer(["Okay."])
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def log_message(self, *_args):
        pass

ThreadingHTTPServer(("127.0.0.1", port), Stub).serve_forever()
PY
SH_STUB=$!
smoke_on_exit kill "$SH_STUB"
for _ in $(seq 1 30); do # wait for the stub to listen
	if python3 -c "import socket, sys; s = socket.socket(); s.settimeout(0.2); sys.exit(0 if s.connect_ex(('127.0.0.1', $SH_PORT)) == 0 else 1)" 2>/dev/null; then
		break
	fi
	sleep 0.1
done
SH_PROVIDERS="$SMOKE_TMP/providers153.toml"
printf '[providers.stub]\nname = "Stub"\n\n[providers.stub.kwargs]\napi_base = "http://127.0.0.1:%s/v1"\n' "$SH_PORT" >"$SH_PROVIDERS"
# A skill for the `skill` call to load: the suite's skills root is empty.
mkdir -p "$SMOKE_SKILLS/web-search"
printf -- '---\nname: web-search\ndescription: Search the web.\n---\nSearch carefully.\n' >"$SMOKE_SKILLS/web-search/SKILL.md"
# Auto mode: the stub's commands are all read-only, so they run unasked and
# nothing but the strip is on screen.
SH_APP="env NO_PROXY=127.0.0.1 no_proxy=127.0.0.1 $CFG_ENV_NOHIST ALTER_ZERO_PERMISSION_MODE=auto ALTER_ZERO_PROVIDERS_FILE=$SH_PROVIDERS ALTER_ZERO_PROVIDER=stub ALTER_ZERO_MODEL=stub-model STUB_API_KEY=stand-in $BIN_ABS"

# ---- 3. "run the chain": quick thoughts, a tail settling, quick rounds -----
S153R="${S}_stripholdchain"
launch -c "$(work_dir chain)" "$S153R" 100 30 "$SH_APP"
submit "$S153R" "run the chain"
watch_turn "$S153R" 'stub-model ·'
note "chain: sampled $W_SAMPLES times; rises=$W_HOPS; longest held run=${W_HELD}ms; block seen=$W_BLOCK"
expect_steady "chain"
expect_eq "$W_BLOCK" "0" "a thought shorter than the show delay put its block up"
chain="$(wait_pane 20 "$S153R" -S -60 -- -F "Both ran.")" || fail "the chain's last round never answered"
dump "the settled chain session" "$chain"
expect_eq "$(grep -cE '^Thought for [0-9]+s' <<<"$chain")" "3" \
	"the chain's three thoughts did not each settle into one 'Thought for' line"
expect_eq "$(grep -cF '● Bash(seq 1 3' <<<"$chain")" "1" "the first call is not there exactly once"
expect_eq "$(grep -cF '● Bash(echo a; echo b)' <<<"$chain")" "1" "the second call is not there exactly once"
keys "$S153R" C-c
wait_gone 10 "$S153R" || fail "the chain session did not quit on Ctrl+C"

# ---- 4. "load the skill": the reported hand-off --------------------------
S153K="${S}_stripholdskill"
launch -c "$(work_dir skill)" "$S153K" 100 30 "$SH_APP"
submit "$S153K" "load the skill"
watch_turn "$S153K" 'stub-model ·'
note "skill: sampled $W_SAMPLES times; rises=$W_HOPS; longest held run=${W_HELD}ms; block seen=$W_BLOCK"
expect_steady "skill"
expect_eq "$W_BLOCK" "0" "a thought shorter than the show delay put its block up"
skill="$(wait_pane 20 "$S153K" -S -60 -- -F "Loaded it.")" || fail "the skill turn never answered"
expect_has "$skill" -F "● Skill(web-search)" "the skill call has no cell"
keys "$S153K" C-c
wait_gone 10 "$S153K" || fail "the skill session did not quit on Ctrl+C"

# ---- 5. "wait for it": a held row lapses instead of lingering ---------------
# The command's clock row is held as it settles, but the next round takes
# four seconds: the row must let go after the hold — one settle, allowed —
# rather than stand over the status line for the whole wait.
S153W="${S}_stripholdwait"
launch -c "$(work_dir wait)" "$S153W" 100 30 "$SH_APP"
submit "$S153W" "wait for it"
watch_turn "$S153W" 'stub-model ·' 1
note "wait: sampled $W_SAMPLES times; rises beyond the lapse=$W_HOPS; longest held run=${W_HELD}ms; held seen=$W_HELD_SEEN"
expect_steady "wait"
expect_eq "$W_HELD_SEEN" "1" "the settling tail's row was never held: nothing to lapse"
wait_pane 20 "$S153W" -S -60 -- -F "Waited." >/dev/null || fail "the slow round never answered"
keys "$S153W" C-c
wait_gone 10 "$S153W" || fail "the wait session did not quit on Ctrl+C"
