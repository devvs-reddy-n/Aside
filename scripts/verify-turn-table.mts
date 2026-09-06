import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DIAGNOSTIC_LAYERS } from "../src/diagnostics/layers.ts";
import {
  NOTEBOOK_COALESCE_MS,
  applyDiskTurns,
  applyLiveTranscript,
  diskAlreadyHas,
  formatChatClock,
  idleTurnTable,
  isLikelySpeakerEcho,
  mergeNotebookText,
  normalizeEchoText,
  echoTokens,
  tokenJaccard,
  ECHO_JACCARD,
  newNotebook,
  originForEngine,
  originLabel,
  shouldContinueLine,
  storedCounts,
  tableRows,
  turnsStayOnTheirEar,
  type StoredTurn,
} from "../src/diagnostics/turnModel.ts";

const here = dirname(fileURLToPath(import.meta.url));
const inspector = readFileSync(join(here, "../src/diagnostics/TurnTable.tsx"), "utf8");
const model = readFileSync(join(here, "../src/diagnostics/turnModel.ts"), "utf8");
const app = readFileSync(join(here, "../src/diagnostics/DiagnosticApp.tsx"), "utf8");
const shortcuts = readFileSync(join(here, "../src-tauri/src/shortcuts.rs"), "utf8");

{
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 3)?.status, "ready");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 3)?.page, "Conversation");
  assert.equal(DIAGNOSTIC_LAYERS.find((l) => l.id === 4)?.status, "ready");
  assert.ok(
    DIAGNOSTIC_LAYERS.filter((l) => l.id >= 9).every((l) => l.status === "not_built"),
    "layers 9–11 stay locked",
  );
}

{
  let id = 0;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = idleTurnTable();
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "My current work authorization is STEM OPT", is_final: false },
    "speechmatics",
    1000,
    nextId,
  );
  assert.equal(state.micInterim, "My current work authorization is STEM OPT");
  assert.equal(state.pending.length, 0, "interim must not become a stored/pending turn");
  assert.equal(state.systemInterim, "");
  assert.equal(state.disk.length, 0);

  state = applyLiveTranscript(
    state,
    "mic",
    {
      text: "My current work authorization is STEM OPT and I work on AWS and Kubernetes.",
      is_final: true,
    },
    "speechmatics",
    1800,
    nextId,
  );
  assert.equal(state.micInterim, "", "final must clear the scratch line");
  assert.equal(state.pending.length, 1);
  assert.equal(state.pending[0].channel, "mic");
  assert.equal(state.systemInterim, "");
  assert.equal(state.pending.filter((p) => p.channel === "system").length, 0);

  state = applyLiveTranscript(
    state,
    "system",
    { text: "Can you explain your Kubernetes experience?", is_final: true },
    "speechmatics",
    2400,
    nextId,
  );
  assert.equal(state.pending.length, 2);
  assert.equal(state.pending[1].channel, "system");
  assert.ok(!state.pending[0].text.includes("Kubernetes experience"));
  assert.ok(!state.pending[1].text.includes("STEM OPT"));
}

{
  assert.equal(NOTEBOOK_COALESCE_MS, 5_000);
  assert.equal(shouldContinueLine("mic", "mic", 1_000, true, "I was thinking"), true);
  assert.equal(shouldContinueLine("mic", "mic", 4_000, true, "I was thinking"), true);
  assert.equal(shouldContinueLine("mic", "mic", 5_000, true, "I was thinking"), true);
  assert.equal(shouldContinueLine("mic", "mic", 5_001, true, "I was thinking"), false);
  assert.equal(shouldContinueLine("mic", "system", 200, true, "hello"), false);
  assert.equal(shouldContinueLine("mic", "mic", 400, true, "Yes."), false);
  assert.equal(shouldContinueLine("mic", "mic", 400, true, "yeah, sure, thanks"), false);
  assert.equal(mergeNotebookText("okay", "okay"), "okay okay");

  let id = 0;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = idleTurnTable();
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "I was thinking", is_final: true },
    "speechmatics",
    1_000,
    nextId,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "about Kubernetes", is_final: true },
    "speechmatics",
    2_000,
    nextId,
  );
  assert.equal(state.pending.length, 1, "1s pause on the same ear must stay one line");
  assert.equal(state.pending[0].text, "I was thinking about Kubernetes");

  state = applyLiveTranscript(
    state,
    "system",
    { text: "Can you explain?", is_final: true },
    "speechmatics",
    2_200,
    nextId,
  );
  assert.equal(state.pending.length, 2, "Others must start a new line");

  state = applyLiveTranscript(
    state,
    "mic",
    { text: "STEM OPT", is_final: true },
    "speechmatics",
    2_400,
    nextId,
  );
  assert.equal(state.pending.length, 3, "other party in between must not glue MY SPEECH");
}

{
  assert.equal(shouldContinueLine("system", "system", 1_500, true, "Can you explain Kubernetes"), true);
  assert.equal(shouldContinueLine("system", "system", 5_000, true, "Can you explain Kubernetes"), true);
  assert.equal(shouldContinueLine("system", "system", 5_001, true, "Can you explain Kubernetes"), false);
  assert.equal(shouldContinueLine("system", "mic", 200, true, "hello"), false);
  assert.equal(shouldContinueLine("system", "system", 400, false, "hello"), false);

  let id = 40;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = idleTurnTable();
  state = applyLiveTranscript(
    state,
    "system",
    { text: "Can you explain Kubernetes", is_final: true },
    "speechmatics",
    1_000,
    nextId,
  );
  state = applyLiveTranscript(
    state,
    "system",
    { text: "in your last role", is_final: true },
    "speechmatics",
    2_500,
    nextId,
  );
  assert.equal(state.pending.length, 1, "1.5s pause on Others must stay one line");
  assert.equal(state.pending[0].text, "Can you explain Kubernetes in your last role");

  state = applyLiveTranscript(
    state,
    "system",
    { text: "What about Helm", is_final: true },
    "speechmatics",
    8_000,
    nextId,
  );
  assert.equal(state.pending.length, 2, "gap over 5s on Others must split");
}

{
  const forty =
    "about a dollar and thirty cents forty cents with kimmy on the phone and we can probably do that if it still makes sense for the budget this quarter";
  const orCents =
    "about a dollar and thirty cents or cents with kimmy on the phone and we can probably do that if it still makes sense for the budget this quarter";
  const longK8s =
    "Can you walk me through Kubernetes and how you used it on STEM OPT last year?";

  assert.equal(isLikelySpeakerEcho("yeah", "yeah"), false, "short yeah/yeah must KEEP");
  assert.equal(isLikelySpeakerEcho("Okay.", "okay"), false);
  assert.ok(
    tokenJaccard(echoTokens(normalizeEchoText(forty)), echoTokens(normalizeEchoText(orCents))) >=
      ECHO_JACCARD,
  );
  assert.equal(isLikelySpeakerEcho(orCents, forty), true, "forty/or screenshot pair must drop");
  assert.equal(isLikelySpeakerEcho(longK8s, longK8s), true);
  assert.equal(
    isLikelySpeakerEcho("I use Kubernetes at work on STEM OPT", longK8s),
    false,
  );

  let id = 80;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = idleTurnTable();
  state = applyLiveTranscript(
    state,
    "system",
    { text: longK8s, is_final: true },
    "speechmatics",
    1_000,
    nextId,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: longK8s, is_final: true },
    "speechmatics",
    1_200,
    nextId,
  );
  assert.equal(state.pending.length, 1, "mic echo of Others must not become a Me bubble");
  assert.equal(state.pending[0].channel, "system");
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "I use Helm at work on STEM OPT", is_final: true },
    "speechmatics",
    1_400,
    nextId,
  );
  assert.equal(state.pending.length, 2, "a real Me line after Others must still land");
  assert.equal(state.pending[1].channel, "mic");
  assert.equal(state.pending[1].text, "I use Helm at work on STEM OPT");

  state = applyLiveTranscript(
    state,
    "system",
    { text: "yeah", is_final: true },
    "speechmatics",
    2_000,
    nextId,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "yeah", is_final: true },
    "speechmatics",
    2_200,
    nextId,
  );
  assert.equal(state.pending.length, 4, "short yeah after Others yeah must KEEP as Me");
  assert.equal(state.pending[3].channel, "mic");
  assert.equal(state.pending[3].text, "yeah");

  // Me lands first (Others STT still catching up), then Others final retro-drops Me.
  let retro = idleTurnTable();
  retro = applyLiveTranscript(
    retro,
    "mic",
    { text: orCents, is_final: true },
    "speechmatics",
    3_000,
    nextId,
  );
  assert.equal(retro.pending.length, 1);
  assert.equal(retro.pending[0].channel, "mic");
  retro = applyLiveTranscript(
    retro,
    "system",
    { text: forty, is_final: true },
    "speechmatics",
    3_400,
    nextId,
  );
  assert.equal(retro.pending.length, 1, "Others final must drop the earlier Me echo");
  assert.equal(retro.pending[0].channel, "system");

  // Barge-in: a different Me sentence while Others talk still splits bubbles.
  let barge = idleTurnTable();
  barge = applyLiveTranscript(
    barge,
    "system",
    { text: longK8s, is_final: true },
    "speechmatics",
    4_000,
    nextId,
  );
  barge = applyLiveTranscript(
    barge,
    "mic",
    { text: "I use Helm at work on STEM OPT", is_final: true },
    "speechmatics",
    4_200,
    nextId,
  );
  assert.equal(barge.pending.length, 2, "other-speaker interruption must still split");
  assert.equal(barge.pending[0].channel, "system");
  assert.equal(barge.pending[1].channel, "mic");
}

{
  const micText = "My current work authorization is STEM OPT and I work on AWS and Kubernetes.";
  const themText = "Can you explain your Kubernetes experience?";
  const disk: StoredTurn[] = [
    {
      id: "a",
      session_id: "s1",
      channel: "mic",
      origin: "speechmatics",
      text: micText,
      start_ms: 1000,
      end_ms: 1800,
      confidence: null,
      is_final: true,
      created_at_ms: 1800,
    },
    {
      id: "b",
      session_id: "s1",
      channel: "system",
      origin: "speechmatics",
      text: themText,
      start_ms: 2000,
      end_ms: 2600,
      confidence: null,
      is_final: true,
      created_at_ms: 2600,
    },
  ];
  assert.ok(turnsStayOnTheirEar(disk, "STEM OPT", "Kubernetes experience"));
  const counts = storedCounts(disk);
  assert.equal(counts.mic, 1);
  assert.equal(counts.system, 1);

  let state = idleTurnTable();
  state = { ...state, sessionId: "s1", pending: [
    { id: 1, channel: "mic", origin: "speechmatics", text: micText, atMs: 1800 },
    { id: 2, channel: "system", origin: "speechmatics", text: themText, atMs: 2400 },
  ] };
  state = applyDiskTurns(state, disk, 3000);
  assert.equal(state.pending.length, 0, "reload must drop pending rows already on disk");
  assert.equal(state.disk.length, 2);
  const rows = tableRows(state, "all");
  assert.equal(rows.length, 2);
  assert.equal(rows[0].channel, "mic");
  assert.equal(rows[1].channel, "system");
  assert.ok(rows.every((r) => r.persisted));
  assert.equal(tableRows(state, "mic").length, 1);
  assert.equal(tableRows(state, "system").length, 1);
  assert.ok(diskAlreadyHas(disk, { id: 1, channel: "mic", origin: "speechmatics", text: micText, atMs: 1 }));
}

{
  assert.equal(
    mergeNotebookText("I was thinking about Kubernetes", "about Kubernetes"),
    "I was thinking about Kubernetes",
  );
  let id = 0;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = applyDiskTurns(
    idleTurnTable(),
    [
      {
        id: "d1",
        session_id: "s",
        channel: "mic",
        origin: "speechmatics",
        text: "I was thinking",
        start_ms: 50,
        end_ms: 1000,
        confidence: null,
        is_final: true,
        created_at_ms: 1000,
      },
    ],
    1000,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "about Kubernetes", is_final: true },
    "speechmatics",
    3000,
    nextId,
  );
  const live = tableRows(state, "all");
  assert.equal(live.length, 1, "a 2s pause on the same ear stays one bubble after disk reload");
  assert.equal(live[0].text, "I was thinking about Kubernetes");
  state = applyDiskTurns(
    state,
    [
      {
        id: "d1",
        session_id: "s",
        channel: "mic",
        origin: "speechmatics",
        text: "I was thinking about Kubernetes",
        start_ms: 50,
        end_ms: 3000,
        confidence: null,
        is_final: true,
        created_at_ms: 3000,
      },
    ],
    3100,
  );
  assert.equal(state.pending.length, 0, "joined disk row absorbs the live fragment");
  assert.equal(tableRows(state, "all").length, 1);
}

{
  let id = 0;
  const nextId = () => {
    id += 1;
    return id;
  };
  let state = applyDiskTurns(
    idleTurnTable(),
    [
      {
        id: "d1",
        session_id: "s",
        channel: "mic",
        origin: "speechmatics",
        text: "I was thinking",
        start_ms: 50,
        end_ms: 1000,
        confidence: null,
        is_final: true,
        created_at_ms: 1000,
      },
    ],
    1000,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "about Kubernetes", is_final: false },
    "speechmatics",
    3_000,
    nextId,
  );
  state = applyLiveTranscript(
    state,
    "mic",
    { text: "about Kubernetes and Helm", is_final: true },
    "speechmatics",
    9_000,
    nextId,
  );
  const live = tableRows(state, "all");
  assert.equal(live.length, 1, "2s think-pause then a long sentence stays one bubble");
  assert.equal(live[0].text, "I was thinking about Kubernetes and Helm");
}

{
  let state = applyDiskTurns(
    idleTurnTable(),
    [
      {
        id: "o",
        session_id: "s",
        channel: "system",
        origin: "speechmatics",
        text: "others first",
        start_ms: 5000,
        end_ms: 5100,
        confidence: null,
        is_final: true,
        created_at_ms: 2000,
      },
      {
        id: "m",
        session_id: "s",
        channel: "mic",
        origin: "speechmatics",
        text: "me spoke last",
        start_ms: 100,
        end_ms: 9000,
        confidence: null,
        is_final: true,
        created_at_ms: 9000,
      },
    ],
    10_000,
  );
  const rows = tableRows(state, "all");
  assert.equal(rows[0].text, "others first");
  assert.equal(rows[1].text, "me spoke last");
  assert.equal(rows[1].channel, "mic", "last spoken Me line must sit at the bottom, not the middle");
}

{
  const noon = new Date("2026-08-25T19:19:51.907Z").getTime();
  const clock = formatChatClock(noon);
  assert.match(clock, /^\d{2}:\d{2}$/);
  assert.ok(!clock.includes("."));
}

{
  const mixed: StoredTurn[] = [
    {
      id: "x",
      session_id: "s1",
      channel: "mic",
      origin: "speechmatics",
      text: "Can you explain your Kubernetes experience?",
      start_ms: 1,
      end_ms: 2,
      confidence: null,
      is_final: true,
      created_at_ms: 2,
    },
  ];
  assert.equal(turnsStayOnTheirEar(mixed, "STEM OPT", "Kubernetes experience"), false);
}

{
  let state = newNotebook(1, "sess-1");
  assert.equal(state.sessionId, "sess-1");
  assert.equal(state.disk.length, 0);
  state = applyLiveTranscript(state, "mic", { text: "   ", is_final: true }, "speechmatics", 2, () => 1);
  assert.equal(state.pending.length, 0, "blank finals are not rows");
  state = applyDiskTurns(
    state,
    [
      {
        id: "zero",
        session_id: "sess-1",
        channel: "mic",
        origin: "speechmatics",
        text: "first line of the call",
        start_ms: 9_000,
        end_ms: 400,
        confidence: null,
        is_final: true,
        created_at_ms: 0,
      },
      {
        id: "interim-should-hide",
        session_id: "sess-1",
        channel: "system",
        origin: "speechmatics",
        text: "should not show",
        start_ms: 10,
        end_ms: 20,
        confidence: null,
        is_final: false,
        created_at_ms: 20,
      },
    ],
    50,
  );
  const rows = tableRows(state, "all");
  assert.equal(rows.length, 1);
  assert.equal(rows[0].atMs, 0, "created_at_ms is the notebook clock, even when start_ms is later");
  assert.equal(rows[0].text, "first line of the call");
}

assert.equal(originLabel("local_parakeet"), "Parakeet");
assert.equal(originForEngine("parakeet_local"), "local_parakeet");
assert.equal(originForEngine("speechmatics"), "speechmatics");

assert.match(app, /page === 3 \? <TurnTable/);
assert.match(inspector, /autoAnswer:\s*false/);
assert.match(inspector, /create_session/);
assert.match(inspector, /list_session_transcripts/);
assert.match(inspector, /start_channel_transcription/);
assert.match(inspector, /listenBoth/);
assert.match(inspector, /Reload/);
assert.match(inspector, /Others/);
assert.match(inspector, /nb-bubble--mic/);
assert.match(inspector, /ListenBar/);
assert.match(inspector, /One button starts you and Others/);
assert.match(inspector, /SttEnginePicker/);
assert.match(inspector, /useSttEngine/);
assert.match(inspector, /restartLiveChannels/);
assert.ok(!inspector.includes("OTHER PARTY"));
assert.ok(!inspector.includes("Layer 3"));
assert.match(shortcuts, /fn diagnostic_cmd_enter_may_call_llm\(\) -> bool/);
assert.match(
  shortcuts,
  /pub fn diagnostic_screen_hotkey_may_call_llm\(\) -> bool \{\n    false\n\}/,
  "Layer 3 must keep screen look-up off",
);
assert.ok(!inspector.includes("sessionId: null"), "Turn table must persist into a real session");
assert.ok(!inspector.includes("sessionId:null"));

for (const banned of ["trigger_answer", "start_session_capture", "looks_like_a_question", "answer-delta"]) {
  assert.ok(!inspector.includes(banned), `Turn table must not call ${banned}`);
  assert.ok(!model.includes(banned), `turnModel must not mention ${banned}`);
}

console.log("verify-turn-table: ok");
