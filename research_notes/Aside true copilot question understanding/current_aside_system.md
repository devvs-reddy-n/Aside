# How Aside understands questions and answers them today (code reading, branch `redesign-dock`, HEAD `abb4293` + uncommitted working tree, read 2026-10-17)

Scope note: every claim below comes from reading the repository source, tests and docs. "Source" links are repo-relative file paths with line numbers (`path:line`). Nothing here was run live; behaviour statements are from code, and anything inferred is marked as an inference. Where docs and code disagree it is called out. The only uncommitted change in the key files is in `src-tauri/src/llm/mod.rs` (typed answers now accept a length override) — the rest of the answer path matches HEAD.

Plain-words summary for the owner: Aside does not try to work out "what the question is" by itself. When you press Command+Enter it grabs **everything said since your last press** (both you and the other person), sends all of it to the AI model with a long rulebook, your resume/job description, and a short running summary of earlier parts of the call, and asks the model "what should I say next?". The rulebook already tells the model to answer several questions together and to treat statements and small talk differently. **But the question shown on screen is just the last sentence the other person said** — a single line of code picks it. That is why you only ever see the last question, even when the answer covers more. No separate step classifies what kind of speech it was, writes a summary of the questions, or tracks follow-ups; the main model is asked to do all of that silently, inside the same request.

---

## 1. End-to-end data flow: audio → speech-to-text → transcript → "window" → prompt → streamed answer → live window

### Takeaway
There is one main answer path: Command+Enter → `llm::respond_to_window` → `window_memory::freeze_for_answer` (take all speech since the last press) → `window_memory::build_request_with_length` (build prompt) → OpenAI streaming → `answer-started` / `answer-delta` / `answer-done` events → `src/live/machine.ts` reducer → `Reading.tsx`. Typed follow-ups and screenshots reuse the same prompt builder, each with a one-line fake conversation instead of the real recent speech.

### Cited Findings
- **Two lanes from capture.** Mic audio is channel `Mic` = speaker "Me"; system (call) audio is channel `System` = speaker "Other" — [window_memory/types.rs:14-20](src-tauri/src/window_memory/types.rs). The live window labels them `me` / `them` — [src/live/machine.ts:175](src/live/machine.ts).
- **STT loop.** `pipeline::run` drains the speech engine every tick or on each audio packet and calls `persist_and_emit` — [src-tauri/src/pipeline.rs:311-453](src-tauri/src/pipeline.rs). `persist_and_emit` first removes mic lines that look like an echo of the other side (`drop_speaker_echo`, `forget_leading_mic_echo`), then saves finals, then calls `emit_all` — [pipeline.rs:474-577](src-tauri/src/pipeline.rs).
- **Fan-out of each transcript event** (`emit_all`, [pipeline.rs:725-780](src-tauri/src/pipeline.rs)): (a) finals are added to the in-memory `transcript_log` (cap 600 turns, [transcript_log.rs:39](src-tauri/src/transcript_log.rs)); (b) **every event, partial or final**, goes to `window_memory::on_transcript`; (c) every event goes to `conversation::on_transcript` (only timing marks now); (d) finals, when a session exists, are indexed for RAG (`rag::indexer::index_final_transcript`) and run through regex fact extraction (`facts::facts_from_utterance`) with results upserted into the `facts` table; (e) the event is sent to the frontend.
- **Stale comment.** `emit_all` says "Partials prepare (prefetch); finals send" — [pipeline.rs:733](src-tauri/src/pipeline.rs). In current code nothing is prefetched or sent automatically; partials are only held as "interim" text so they can be included at the moment of the press (see §6).
- **Speechmatics utterance boundaries.** Speechmatics deltas stay "interim" and become one final only at `EndOfUtterance` — [stt/speechmatics_live.rs:226-283](src-tauri/src/stt/speechmatics_live.rs). End-of-utterance fires after `EOU_SILENCE_TRIGGER = 0.6` s of silence — [stt/speechmatics.rs:88-89](src-tauri/src/stt/speechmatics.rs). So an interviewer who pauses ~0.6 s between two questions produces two separate "Other" turns.
- **Live conversation window.** `window_memory::on_transcript` → `live::apply_event` keeps finals as turns in `open` and keeps the latest interim per speaker — [window_memory/mod.rs:68-90](src-tauri/src/window_memory/mod.rs), [window_memory/live.rs:67-113](src-tauri/src/window_memory/live.rs).
- **Shortcut wiring.** Global Command+Enter → `shortcuts::answer_current_question` → `llm::respond_to_window(app, session_id, None)` — [shortcuts.rs:205, 221-232](src-tauri/src/shortcuts.rs). The live window's Answer / Short / Med / Long buttons call `trigger_window_answer` → `respond_to_window` — [commands/session.rs:165-171](src-tauri/src/commands/session.rs); [src/live/controller.ts:135-137](src/live/controller.ts).
- **`respond_to_window`** claims the answer slot, freezes the window, queues the window for background summarising (only when it is new), then calls `window_answer_with_claim` — [llm/mod.rs:1038-1067](src-tauri/src/llm/mod.rs).
- **`window_answer_with_claim`** picks the displayed "headline", emits `answer-started`, builds the prompt, streams through the provider chain via `WindowAnswerSink`, then records diagnostics with `window_memory::note_press` — [llm/mod.rs:1160-1234](src-tauri/src/llm/mod.rs).
- **Frontend.** `backend.ts` maps `answer-started` / `question-cleaned` / `answer-delta` / `answer-done` / `answer-error` into reducer actions — [src/live/backend.ts:105-109](src/live/backend.ts). The reducer creates an `Answer` item whose `question` is the event's text — [src/live/machine.ts:207-235](src/live/machine.ts). `Reading.tsx` shows `cleanedQuestion ?? question` — [src/live/parts/Reading.tsx:19-21](src/live/parts/Reading.tsx).
- **Typed follow-up** ("Ask a follow-up question" field) → `trigger_answer` → `respond_to_question` → `answer_with_claim`, which wraps the typed text as **one synthetic "Me" turn** and builds the prompt with that alone — [llm/mod.rs:1022-1033, 1070-1112](src-tauri/src/llm/mod.rs); [src/live/parts/Composer.tsx:27-31](src/live/parts/Composer.tsx).
- **Screenshot** (Command+Shift+Enter) uses `transcript_log::last_them_turn` as "the question" — [llm/mod.rs:1294](src-tauri/src/llm/mod.rs) — wrapped as a single synthetic "Other" turn by `build_full_context` — [llm/mod.rs:1254-1281](src-tauri/src/llm/mod.rs).

### Inferences
- In plain words: there is a "bucket" of speech that fills up between presses. Pressing the key empties the bucket into one AI request. The AI model does all the understanding; Aside's own code does almost none.

### Gaps
- The live answer path was not run here (no real call, no API key used). Timing claims come from docs, not measured in this pass.

---

## 2. Exactly how "the question" is chosen, and why only the last of several questions appears

### Takeaway
There is no question-selection step. The **whole** window (all turns since the previous press) goes to the model, and the rules tell it to answer every open question together. But the question **displayed** is picked by one line: the text of the **last "Other" turn** in the window (or the last turn of any speaker if the other person said nothing). The window path never produces a "cleaned"/combined question. That is the direct cause of complaint (1).

### Cited Findings
- **Window = everything since the last press.** `Inner::freeze_for_answer` takes the window start from `open_started_ms` (set when the first speech after the previous press arrived) and freezes all committed turns plus the current interims — [window_memory/mod.rs:106-121](src-tauri/src/window_memory/mod.rs); `live::freeze_with_live` adds the interim text, sorts by time and empties `open` — [window_memory/live.rs:117-151](src-tauri/src/window_memory/live.rs). Module doc: "Everything spoken between two presses is one raw window. The window is frozen at the press; later speech belongs only to the next window." — [window_memory/mod.rs:1-4](src-tauri/src/window_memory/mod.rs).
- **Empty press replays the last window.** If nothing new was said, the previous window is reused (`is_new: false`, not queued for summary again) — [window_memory/mod.rs:114-120](src-tauri/src/window_memory/mod.rs); tested by `repeat_answer_and_all_lengths_reuse_the_consumed_window` and `new_speech_replaces_the_replay...` — [window_memory/mod.rs:399-430](src-tauri/src/window_memory/mod.rs).
- **The displayed headline (verbatim):**
  ```rust
  // Name the actual thing being answered. A fixed label made Command+Enter
  // answers look as if they had no question at all.
  let headline = window.turns.iter().rev()
      .find(|turn| matches!(turn.speaker, crate::window_memory::Speaker::Other))
      .or_else(|| window.turns.last())
      .map(|turn| turn.text.clone())
      .unwrap_or_else(|| "Current conversation".to_string());
  emit_answer_started_if_live(&app, generation, headline.clone());
  ```
  — [llm/mod.rs:1170-1180](src-tauri/src/llm/mod.rs). The same `headline` is passed as the provider's "question" argument (used only for labels/events; the real prompt is the encoded `system`/`user` pair) — [llm/mod.rs:1197-1198](src-tauri/src/llm/mod.rs).
- **No cleaned question on the window path.** The window path wraps the stream in `WindowAnswerSink` (plain pass-through) — [llm/mod.rs:633-718](src-tauri/src/llm/mod.rs), not `SplittingSink`, so `question-cleaned` is never emitted for Command+Enter answers. The UI therefore always shows the raw last-Other line: `page.answer.cleanedQuestion ?? page.answer.question` — [src/live/parts/Reading.tsx:21](src/live/parts/Reading.tsx); `cleanedQuestion` starts `null` — [src/live/machine.ts:216](src/live/machine.ts).
- **Where "question cleaning" exists, it is a leftover from an older design.** `SYSTEM_PROMPT` (2026-08-25) told the model to start with `Q: <clean restatement>` then `A: <answer>` — [llm/prompt.rs:19-41](src-tauri/src/llm/prompt.rs); `QuestionAnswerSplitter` parses the `A:` marker, with a 300-char cap — [llm/question_split.rs:19-78](src-tauri/src/llm/question_split.rs); `SplittingSink` emits `question-cleaned` — [llm/mod.rs:538-605](src-tauri/src/llm/mod.rs). But every current caller sends the window-memory prompt via `encode_messages`, so `compose_system_and_user` uses `STRICT_RULES`, not `SYSTEM_PROMPT` — [llm/prompt.rs:46-62](src-tauri/src/llm/prompt.rs). `STRICT_RULES` ends: "Write only the visible answer, as plain text I can say out loud — nothing else. No JSON, no markdown fences, no metadata block, no preamble..." — [window_memory/prompt.rs:169-172](src-tauri/src/window_memory/prompt.rs).
- **Typed follow-ups still go through `SplittingSink`** — [llm/mod.rs:1125, 1134](src-tauri/src/llm/mod.rs) — while the prompt now asks for plain text with no `Q:`/`A:`.
- **The rules do ask the model to answer all open questions.** Rule 2: "Read the entire Current Conversation Window before responding. Never blindly answer only the final sentence." Rule 7: "If multiple unanswered questions belong together, answer them naturally as one response." Example 4 ("Tell me about your current project." / "And what do you expect from this position?"): "address both, in order, rather than answering only the last one." — [window_memory/prompt.rs:9, 22, 151-155](src-tauri/src/window_memory/prompt.rs). Tests confirm the whole window reaches the prompt (`prompt_contains_the_whole_window_not_only_the_last_line`) — [window_memory/prompt.rs:620-640](src-tauri/src/window_memory/prompt.rs).
- **Two backend merges can swallow a question before it reaches the window.** `push_turn` replaces the previous turn instead of adding a new one when the same speaker's new text "is a restatement" — [window_memory/live.rs:48-65](src-tauri/src/window_memory/live.rs). `is_restatement` = one text starts with the other **or** word-set overlap (Jaccard) ≥ 0.6 — [conversation/similarity.rs:41-50](src-tauri/src/conversation/similarity.rs).
- **Frontend "Question detected" card is also last-only and fragment-only.** On each final from `them`, `question = looksLikeQuestion(text) ? { turnId, text } : state.question` — so each new question-like final replaces the previous one, and `text` is only the new final piece, even when the reducer merged it into a longer on-screen turn (finals within 6 s join, `MERGE_WINDOW_MS = 6000`) — [src/live/machine.ts:130, 184-198](src/live/machine.ts). Only that one pending question becomes a page — [src/live/navigation.ts:5-10](src/live/navigation.ts).

### Inferences
- Plain words: the AI already sees all the questions and is told to answer all of them, but the screen labels the answer with only the last sentence the interviewer said. If the interviewer asked "Tell me about your project" … pause … "and what did you learn?", the label reads "and what did you learn?" even if the answer covers both.
- Two similar consecutive questions from the interviewer, such as "What is your notice period?" followed by "What is your notice period and expected salary?", would be stored as **one** turn (the second starts with the first, so it replaces it). That is usually fine. But the word-overlap ≥ 0.6 rule could in principle merge two genuinely different short questions that share most words (e.g. "How big was the team?" / "How big was the budget?" shares 4 of 6 distinct words = 0.67). Inferred from the code; not tested.
- Typed follow-ups: because the prompt asks for plain text, the model normally never writes "A:". In that case `SplittingSink` holds back the first ~300 characters of the answer before showing anything (until the cap releases it), which adds visible delay. And if a typed answer contains "A:" in its first 300 characters (e.g. "Plan A: …"), the text before it would be shown as the "question" and cut from the answer. Inferred from `question_split.rs` + `STRICT_RULES`; not reproduced live.
- Fixing complaint (1) needs **some step that writes the question label** (e.g. the model streams a short combined-question line before the answer, or a fast parallel call summarises the open questions). Today neither exists for the window path.

### Gaps
- Whether the live model actually answers every question when the window holds several was not measured; `docs/evidence.md` was not checked for a real multi-question run.

---

## 3. What kinds of speech Aside tells apart today (classification)

### Takeaway
Aside's own code has **no utterance classifier**. Every distinction (question, multi-part question, follow-up, statement/information, acknowledgment, small talk, code request) is delegated to the main model through prompt rules, inside the same request that writes the answer, with no structured output. The only code-level "classifiers" are crude word lists used for UI hints and timing, plus filters for "okay"-only turns and speaker echo. Auto-answer was deleted.

### Cited Findings
- **Prompt-level intent list.** Rule 4: "Determine the current conversational intent first: unanswered question, follow-up question, clarification, request for detail, interviewer explanation, acknowledgment, topic transition, multiple related questions" — [window_memory/prompt.rs:11-19](src-tauri/src/window_memory/prompt.rs). Rule 55: "Classify the interviewer's most recent turn before answering: a genuine question needing an answer, a follow-up or clarification, purely sharing information or context with no question attached, or a request to write/debug code or design/explain a system..." — [prompt.rs:97](src-tauri/src/window_memory/prompt.rs). Rule 56 (sharing information → brief conversational reply, "Do not manufacture a question that was never asked") — [prompt.rs:99](src-tauri/src/window_memory/prompt.rs). Rule 28: "For follow-up questions, answer only the missing detail instead of restarting the entire explanation." — [prompt.rs:62](src-tauri/src/window_memory/prompt.rs). Rule 5 / Example 3: don't re-answer what "Me" already answered — [prompt.rs:20, 144-149](src-tauri/src/window_memory/prompt.rs).
- **Internal contradiction in the rules.** Rule 2 says never answer only the final sentence, but rule 55 says "Classify the interviewer's **most recent turn**" and lets that classification "decide both the shape and the length" — [prompt.rs:9, 97](src-tauri/src/window_memory/prompt.rs). Rule 29 mentions "If a specific target question has been identified", but no code ever identifies one; the prompt builder's doc says a "target question" section "is a later phase's job — deliberately not built here" — [prompt.rs:63, 488-495](src-tauri/src/window_memory/prompt.rs).
- **Rule 9** forbids "No question detected." / "No response required." — [prompt.rs:24-28](src-tauri/src/window_memory/prompt.rs) — so the model always produces something to say, even for small talk.
- **Decision history.** The question-type rules (55-58) were added 2026-08-31 at the owner's request to tell apart a real question, a plain statement, and a coding/design request — [docs/decisions.md:104-110](docs/decisions.md).
- **Code-level heuristics (none choose what to answer):**
  - Frontend `looksLikeQuestion` (ends with "?", starts with an opener word such as what/how/can, or a prompt phrase such as "tell me", "walk me through"; ignores filler-only lines) — only decides whether to show the "Question detected" card — [src/core/questions.ts:1-42](src/core/questions.ts).
  - Backend `llm::trigger::looks_like_a_question` (ends with "?" or starts with an opener word) — [llm/trigger.rs:24-50](src-tauri/src/llm/trigger.rs); `conversation::hypothesis` builds on it, but it only sets a timing mark (`mark_identifiable`) — [conversation/hypothesis.rs:1-60](src-tauri/src/conversation/hypothesis.rs); [conversation/mod.rs:87-89](src-tauri/src/conversation/mod.rs).
  - `conversation::ack::is_ack_only` drops whole-turn "okay / got it / thanks..." from the conversation buffers (not from the window) — [conversation/ack.rs:1-50](src-tauri/src/conversation/ack.rs); [conversation/mod.rs:70-73](src-tauri/src/conversation/mod.rs).
  - Speaker-echo filtering removes mic lines that duplicate what the other side just said (speaker bleed) — [pipeline.rs:474-520](src-tauri/src/pipeline.rs).
  - STRICT_RULES "TRANSCRIPTION NOISE" section asks the model to ignore stray fragments but keep short real answers like "Yes." — [prompt.rs:105-142](src-tauri/src/window_memory/prompt.rs).
- **Auto-answer deleted (confirmed in code and docs).** `conversation/mod.rs` doc: the auto-answer mode "was removed (2026-08-31, owner request ...) along with the question/restart/continue detection that only existed to serve it" — [conversation/mod.rs:4-16](src-tauri/src/conversation/mod.rs); `decide()` now only updates buffers and timing marks — [conversation/mod.rs:58-93](src-tauri/src/conversation/mod.rs); [docs/decisions.md:49-51](docs/decisions.md); [status.md:61](status.md).
- **Tables that look like classification but are not fed.** `storage::exchanges` (interview_exchanges) has `open` / `set_user_answer` / `set_ai_suggestion`, but nothing outside `storage/` calls them; only `list_recent` (from `conversation::refresh_topic`) and `list_all` (past-session view) read it — [storage/exchanges.rs:54-158](src-tauri/src/storage/exchanges.rs); [conversation/mod.rs:135-146](src-tauri/src/conversation/mod.rs); [commands/past_sessions.rs:135](src-tauri/src/commands/past_sessions.rs). `note_ai_suggestion` (writes `generated_suggestions` + refreshes `topic_summaries` from those empty exchanges) is called only from the typed path, not from Command+Enter — [llm/mod.rs:1150-1155](src-tauri/src/llm/mod.rs); [conversation/mod.rs:119-133](src-tauri/src/conversation/mod.rs).
- **Docs vs code.** CONTEXT.md describes `conversation/` as "both ears, Prepare-only while auto-answer is off" — [CONTEXT.md:13](CONTEXT.md) — and decisions.md says hypothesis.rs "only prepares" — [docs/decisions.md:51](docs/decisions.md). In code nothing is prepared: no speculative request is ever sent (see §6).

### Inferences
- Plain words: Aside asks one AI model to silently decide "is this a question, a follow-up, small talk, or just information?" **and** write the answer, all at once, with nothing written down about that decision. Because the decision is never written down, the app can't show it, can't use it to label the screen, can't remember "question 3 is still open", and can't treat a follow-up as linked to an earlier question.
- There is no concept of a stored "open question" or "question thread" anywhere in code. Follow-ups rely on the model reading the 1-2 sentence summaries of earlier windows (see §4).
- The user's own speech ("Me") is never classified (e.g. "Me asked the interviewer a question" vs "Me answered"). The rules only say Me statements are authoritative claims — [prompt.rs:30, 36](src-tauri/src/window_memory/prompt.rs).

### Gaps
- No evaluation set or tests exist for how well the model follows the classification rules; tests only check that text reaches the prompt.

---

## 4. Personalisation and memory: what goes into the answer prompt, in what order, with what size limits, and which models

### Takeaway
Personalisation is "paste the documents into the system message": rules, then a DEFAULT CONTEXT block with Notes (copilot Guidance), Company, Job description (≤6,000 chars), Resume (≤6,000 chars), and other documents (≤8,000 chars total). The user message holds Known Facts, a running list of 1-2 sentence window summaries, and the raw window. RAG retrieval is **not** used for live answers. The target role is **not** in the prompt. Only OpenAI is enabled; default answer model `gpt-5.6-luna`, background summariser `gpt-5.4-mini`, screenshots `gpt-5.6-terra`.

### Cited Findings
- **Prompt order (verbatim structure)** from `assemble` — [window_memory/prompt.rs:555-577](src-tauri/src/window_memory/prompt.rs):
  - `system` = effective rules (built-in `STRICT_RULES` or the Settings override) + `DefaultContext::render()` + (if Short/Med/Long pressed) the `ANSWER LENGTH OVERRIDE` directive.
  - `user` = `SessionMemory::render()` ("KNOWN FACTS" list, then "CONVERSATION SO FAR (oldest first, one line per completed exchange)") + `NEW RAW CONVERSATION WINDOW` + the window as `Other: …` / `Me: …` lines + "Now determine the most appropriate thing I should say at this exact point in the conversation."
  - Rendering of memory: [window_memory/types.rs:87-110](src-tauri/src/window_memory/types.rs); window lines: [types.rs:37-61](src-tauri/src/window_memory/types.rs).
- **DEFAULT CONTEXT contents and caps** — `[Notes]` (session instructions), `[Company]`, `[Job description]` truncated to 6,000 chars, `[Resume]` truncated to 6,000 chars, other docs each ≤4,000 and ≤8,000 total, or "(none loaded)" — [prompt.rs:393-462](src-tauri/src/window_memory/prompt.rs).
- **Overall budget ladder.** `CONTEXT_CHAR_BUDGET = 48_000` chars — [prompt.rs:219-221](src-tauri/src/window_memory/prompt.rs). If over: drop other docs → drop oldest summaries one by one → cut resume and JD to 2,000 chars each — [prompt.rs:514-553](src-tauri/src/window_memory/prompt.rs). The raw window is never cut.
- **Where the context comes from.** `load_default_context_from_store`: company / JD / instructions from the active session; rules override from settings; instructions fall back to the `instructions` table; documents attached to the session (first `resume` wins, `job_description` doc only if the session had no JD text, everything else is "other") — [window_memory/mod.rs:286-337](src-tauri/src/window_memory/mod.rs). Cached per session and warmed at session start (`warm_defaults`) — [window_memory/mod.rs:153-171](src-tauri/src/window_memory/mod.rs).
- **Copilot → session.** `start_copilot_call` copies company, JD, instructions ("Guidance"), attached documents, model choice; ingests a regex "knowledge pack" from each document; RAG-indexes each document — [commands/copilots.rs:195-273](src-tauri/src/commands/copilots.rs). `target_role` is used only as the **session title** (`Session::new(copilot.target_role...)`), and `DefaultContext` has no role field — [commands/copilots.rs:212-218](src-tauri/src/commands/copilots.rs); [prompt.rs:297-312](src-tauri/src/window_memory/prompt.rs).
- **Known Facts.** DB facts (`key: value`) are loaded into memory only inside `warm_defaults`, which is called after session create, copilot start, document actions and rule changes — [window_memory/mod.rs:159-183](src-tauri/src/window_memory/mod.rs); callers listed by grep: `commands/documents.rs:232,327,410`, `commands/copilots.rs:272,352`, `commands/session.rs:78`. Facts come from narrow regexes for 4 keys only: `visa_status`, `notice_period`, `title`, `location` — [facts/extract.rs:1-20, 45-102](src-tauri/src/facts/extract.rs). Document facts come from the same regexes ("No extra LLM") — [facts/pack.rs:1-30](src-tauri/src/facts/pack.rs).
- **Spoken facts don't reach the live prompt mid-call.** `emit_all` saves facts extracted from speech to the DB — [pipeline.rs:745-770](src-tauri/src/pipeline.rs) — but does not call `warm_defaults`, so they reach `SessionMemory` only if some later document/rules action re-warms it (inferred from the caller list above).
- **Fact de-dupe is substring-based.** `merge_db_facts` → `push_unique` skips exact or substring matches, so a corrected value ("visa_status: H-1B" after "visa_status: OPT") is **added next to** the old line rather than replacing it in memory — [window_memory/merge.rs:22-46, 66-70](src-tauri/src/window_memory/merge.rs). (The DB upsert itself replaces — [facts/extract.rs:16-19](src-tauri/src/facts/extract.rs).)
- **Conversation memory = one 1-2 sentence summary per pressed window.** After each fresh freeze, the window is queued to a single background worker — [llm/mod.rs:1053-1065](src-tauri/src/llm/mod.rs); [window_memory/background.rs:1-34](src-tauri/src/window_memory/background.rs). The summariser prompt: "Write exactly 1 or 2 plain prose sentences describing only what was actually said in this window ... Do not answer any question in it" — [llm/background.rs:30-35](src-tauri/src/llm/background.rs). It never sees the AI's answer, by design — [llm/background.rs:7-18](src-tauri/src/llm/background.rs). The list grows without a cap; only the prompt budget trims it — [window_memory/types.rs:64-85](src-tauri/src/window_memory/types.rs); [merge.rs:48-56](src-tauri/src/window_memory/merge.rs).
- **RAG is not in the live answer.** Retrieval constants and `build_context_string` are `#[cfg(test)]` only — [llm/mod.rs:111-114, 980-1012](src-tauri/src/llm/mod.rs); no `fn retrieve_context` exists in `src-tauri/src` (grep). The window path doc: "one request, no RAG" — [llm/mod.rs:1034-1037](src-tauri/src/llm/mod.rs); a test asserts "window path must not dump RAG" — [prompt.rs:636-639](src-tauri/src/window_memory/prompt.rs). Documents and transcripts are still indexed (`rag::indexer`), but nothing reads that index while answering. `llm/prompt.rs` and `rag/retriever.rs` doc comments still describe retrieval as wired — [llm/prompt.rs:1-17](src-tauri/src/llm/prompt.rs); [rag/retriever.rs:10-13](src-tauri/src/rag/retriever.rs) — stale.
- **Session instructions / guidance** can be changed mid-call (`set_session_instructions`) — [commands/session.rs:94](src-tauri/src/commands/session.rs) — and appear as `[Notes]`.
- **Strict rules** are user-editable in Settings (stored under `answer_rules_override`), validated only as non-empty — [window_memory/mod.rs:40-44, 229-267](src-tauri/src/window_memory/mod.rs); [prompt.rs:279-295](src-tauri/src/window_memory/prompt.rs).
- **Answer length.** AUTO bands in rule 25 (acknowledgment 1-2 sentences; simple factual 15-40 words; normal 40-90; deep dive 90-180) — [prompt.rs:55-61](src-tauri/src/window_memory/prompt.rs). Short/Med/Long add an override directive with the same bands — [prompt.rs:174-217](src-tauri/src/window_memory/prompt.rs). **Doc/code mismatch:** decisions.md says the override is "appended only to the `user` half of the prompt, never `system`" — [docs/decisions.md:113](docs/decisions.md) — but the code appends it to `system` — [prompt.rs:572-575](src-tauri/src/window_memory/prompt.rs), and a test asserts it is in `system` — [prompt.rs:584-598](src-tauri/src/window_memory/prompt.rs).
- **Providers/models.** `LlmProviderKind::ENABLED = [OpenAi]` (Anthropic/Gemini/Groq code exists but is disabled) — [llm/mod.rs:145](src-tauri/src/llm/mod.rs). OpenAI `DEFAULT_MODEL = "gpt-5.6-luna"`; presets `gpt-5.6-luna, gpt-5.6-terra, gpt-5.5, gpt-5.4, gpt-5.4-mini`; `BACKGROUND_MODEL = "gpt-5.4-mini"`; `VISION_MODEL = "gpt-5.6-terra"` — [llm/openai.rs:24-58](src-tauri/src/llm/openai.rs). GPT-5.x models are sent `reasoning_effort: "none"` — [openai.rs:72-98](src-tauri/src/llm/openai.rs). Request body: Chat Completions, `stream: true`, system + user messages, `prompt_cache_key` from the system text; no max-tokens or temperature — [openai.rs:153-169](src-tauri/src/llm/openai.rs). A started Copilot fixes the model for that call — [llm/mod.rs:827-842](src-tauri/src/llm/mod.rs). The background summariser is pinned to OpenAI — [llm/background.rs:52-88](src-tauri/src/llm/background.rs).

### Inferences
- Plain words: "personalised" today means the AI has your resume, the job description, the company name and your Guidance notes pasted in front of every question, plus a tiny list of facts (visa, notice period, title, location). It does not look up the most relevant bits of your documents for each question, it doesn't know the role title unless the job description says it, and it does not build up a picture of you from what you say during the call (beyond those four regex facts, which in practice don't even reach the prompt mid-call).
- Long documents are cut off at 6,000 characters (roughly 1,000 words) — a longer resume loses its later sections, with no choice about what is kept.
- The summary memory is very lossy: a ten-minute stretch between presses (for example the interviewer explaining the team and product) becomes one or two sentences. If the owner never presses, nothing is summarised at all; the open window just keeps growing and is sent whole on the next press.

### Gaps
- Whether OpenAI prompt caching actually triggers (cache hits) was not checked in this pass; `docs/evidence.md` may hold measurements.
- How the five-bullet "AI summary" shown for JD/Guidance in Copilot setup (`llm/setup_summary.rs`) is used — the CONTEXT doc says it is display-only and "the original remains the session context" — [CONTEXT.md:16](CONTEXT.md); not separately verified in code.

---

## 5. How the Me (mic) and Others (system) lanes are used

### Takeaway
Both lanes go into the window as labelled lines (`Me:` / `Other:`) in time order, and the model is told the user's spoken words are authoritative. Me speech is used for "already answered" detection and corrections. The displayed headline ignores Me unless Other said nothing. Typed questions are disguised as "Me" speech.

### Cited Findings
- Window lines are rendered as `Me: …` / `Other: …` — [window_memory/types.rs:22-41](src-tauri/src/window_memory/types.rs); both speakers' interims are included at freeze — [window_memory/live.rs:117-151](src-tauri/src/window_memory/live.rs).
- Rules: priority list puts "latest actual user-spoken clarification/correction" first; "Actual spoken user statements are authoritative"; "AI-generated answers must never become factual memory merely because they were displayed" — [prompt.rs:29-37](src-tauri/src/window_memory/prompt.rs). Worked examples use Me lines to spot already-answered questions and noise ("October") — [prompt.rs:130-149](src-tauri/src/window_memory/prompt.rs). Test `case_c_responsibility_is_still_in_the_same_window_as_answered_tools` keeps Me answers in the window — [window_memory/mod.rs:503-545](src-tauri/src/window_memory/mod.rs).
- Mic lines that look like an echo of the other side are dropped before they reach the window — [pipeline.rs:474-520, 545-577](src-tauri/src/pipeline.rs).
- Headline prefers the last Other turn — [llm/mod.rs:1172-1179](src-tauri/src/llm/mod.rs).
- Typed asks become a single synthetic `Speaker::Me` turn — [llm/mod.rs:1097-1112](src-tauri/src/llm/mod.rs); screenshot uses the last "them" line as a synthetic `Other` turn — [llm/mod.rs:1254-1281, 1294](src-tauri/src/llm/mod.rs).
- Memory summaries describe both lanes ("Other" and "Me" labels) — [llm/background.rs:30-35](src-tauri/src/llm/background.rs).
- Reference comparison (see §8): the reference app **drops the user's own mic** from the question slice whenever system audio exists, and drops lines that echo a recent AI answer — [references/src/main/services/TranscriptStore.ts:77-115](references/src/main/services/TranscriptStore.ts).

### Inferences
- Because a typed question is labelled "Me" exactly like real speech, the model cannot tell "the user is asking me privately for help" from "the user said this out loud in the meeting". Rule 11 then treats it as something the user "claimed during the interview". Inferred risk; not tested.
- If the user asks the interviewer a question (Me lane), the model may still try to "answer" it, because nothing marks who is asking whom. Inferred.

### Gaps
- No tests cover a window where Me asks a question or makes small talk.

---

## 6. Latency path: prefetch, speculation, caching

### Takeaway
There is no prefetch and no speculative generation today: the answer request starts only when the key is pressed. Speed comes from (a) including live interim words at press time so the user doesn't wait for the final transcript, (b) a pooled/pre-warmed HTTP connection, (c) a stable system prefix with `prompt_cache_key` for OpenAI prompt caching, (d) `reasoning_effort: "none"`, and (e) the memory summary running in the background after the answer starts.

### Cited Findings
- Freeze includes the latest interim "without waiting for a final" — [window_memory/live.rs:1-6, 186-208](src-tauri/src/window_memory/live.rs).
- Background summary is enqueued and "never awaited" before or after the answer — [llm/mod.rs:1053-1065](src-tauri/src/llm/mod.rs); [window_memory/background.rs:17-20](src-tauri/src/window_memory/background.rs).
- Pooled `reqwest::Client` and `spawn_prewarm` — [llm/mod.rs:719-760](src-tauri/src/llm/mod.rs).
- `prompt_cache_key` on the system text and `PROMPT_CACHE_TOKEN_FLOOR = 1_024` — [openai.rs:164](src-tauri/src/llm/openai.rs); [prompt.rs:223-228](src-tauri/src/window_memory/prompt.rs). The system prefix (rules + documents) stays the same for the session; the Short/Med/Long override is appended at the **end** of `system`, so the shared prefix before it is unchanged — [prompt.rs:572-575](src-tauri/src/window_memory/prompt.rs).
- `begin_or_supersede` is documented "Used when a partial-question guess was wrong" — [llm/mod.rs:925-938](src-tauri/src/llm/mod.rs) — but its only callers are user actions (`try_begin_user_answer`); nothing guesses — vestigial. `trigger.rs` still says "Speculative auto-answers do not write this clock" — [llm/trigger.rs:52-54](src-tauri/src/llm/trigger.rs) — stale.
- The "Partials prepare (prefetch)" comment in `emit_all` — [pipeline.rs:733](src-tauri/src/pipeline.rs) — and the decisions note that hypothesis.rs "only prepares" — [docs/decisions.md:51](docs/decisions.md) — do not match code: `hypothesis::is_sufficiently_understood` only sets a timing mark — [conversation/mod.rs:87-89](src-tauri/src/conversation/mod.rs).
- Typed answers add delay because `SplittingSink` holds text until "A:" or 300 characters (see §2) — [llm/question_split.rs:22-26, 51-78](src-tauri/src/llm/question_split.rs).

### Inferences
- Any new "understand the question first" step (classification, combined question) would sit on this hot path. Options to keep latency flat would be: doing the understanding continuously in the background on finals (so a structured "open questions" state is ready before the press), or streaming a short question line at the start of the same answer request.

### Gaps
- Real time-to-first-word numbers were not measured in this pass.

---

## 7. Gaps and failure modes versus the owner's goals

### Takeaway
The code meets only part of the "true copilot" goal through prompt wording. Structurally it is missing: a question label that covers every question asked, an explicit and visible classification, tracking of open questions and follow-ups across presses, the previous answer as context for follow-ups, richer personalisation (role, relevant document parts, a profile built from the call), and pre-press understanding.

### Cited Findings (each a code fact; impact is inference)
1. **Only the last question is displayed** — headline = last Other turn; no combined/cleaned question on the window path — [llm/mod.rs:1170-1180](src-tauri/src/llm/mod.rs); [Reading.tsx:21](src/live/parts/Reading.tsx).
2. **"Question detected" card keeps only the latest question-like final, and only that final's piece of text** — [machine.ts:197](src/live/machine.ts).
3. **No stored classification** — intent is decided silently inside the answer call (rules 4, 55) — [prompt.rs:11-19, 97](src-tauri/src/window_memory/prompt.rs). Nothing records "this was small talk / a statement / a follow-up to Q2".
4. **Contradictory instructions** — "never answer only the final sentence" vs "classify the interviewer's most recent turn" — [prompt.rs:9, 97](src-tauri/src/window_memory/prompt.rs). Very long rulebook (58 rules + 6 examples) in one prompt.
5. **Follow-ups across presses depend on a 1-2 sentence summary written in the background** — [llm/background.rs:30-35](src-tauri/src/llm/background.rs). On a quick second press, the previous window's summary may not have landed yet (async queue, [window_memory/background.rs:21-28](src-tauri/src/window_memory/background.rs)), and the previous window's raw text is no longer in the new window (`freeze_open` empties it, [merge.rs:72-80](src-tauri/src/window_memory/merge.rs)). Inference: "Can you go deeper on that?" right after an answer can lose what "that" was.
6. **The model never sees its own previous answer** (by design, so unspoken suggestions don't become "facts") — [llm/background.rs:7-18](src-tauri/src/llm/background.rs). Inference: typed requests like "shorter", "another example", "continue" cannot work well. The reference app has an explicit "continue the previous answer" prompt that restates the original question — [references/src/main/services/ChatService.ts:67-85](references/src/main/services/ChatService.ts).
7. **Typed follow-ups ignore recent live speech** — the prompt holds only the typed text as one "Me" turn plus memory summaries; the current open window and the last answered window are not included — [llm/mod.rs:1090-1112](src-tauri/src/llm/mod.rs).
8. **Typed path format mismatch** — `SplittingSink` waits for a `Q:/A:` format the current rules forbid — [llm/mod.rs:1125](src-tauri/src/llm/mod.rs); [prompt.rs:169-172](src-tauri/src/window_memory/prompt.rs).
9. **Small talk can't get a "nothing needed" result** — rule 9 forbids "No response required" — [prompt.rs:24-28](src-tauri/src/window_memory/prompt.rs); the UI always shows an answer card for every press.
10. **Personalisation gaps** — target role not in prompt ([copilots.rs:212-218](src-tauri/src/commands/copilots.rs)); resume/JD hard-cut at 6,000 chars ([prompt.rs:425, 436](src-tauri/src/window_memory/prompt.rs)); no per-question retrieval (RAG unused live, [llm/mod.rs:111-114](src-tauri/src/llm/mod.rs)); facts limited to 4 regex keys ([facts/extract.rs:45-102](src-tauri/src/facts/extract.rs)); spoken facts not re-loaded mid-call; corrected facts kept next to stale ones in memory ([merge.rs:26-46](src-tauri/src/window_memory/merge.rs)). No user profile or "story bank" beyond the raw documents.
11. **Memory only advances on presses** — windows are summarised only when frozen — [llm/mod.rs:1063-1065](src-tauri/src/llm/mod.rs). Long unpressed stretches are sent raw (no cap on the window) or compressed hard into 1-2 sentences.
12. **Same-speaker merging** may collapse similar consecutive questions — [live.rs:48-65](src-tauri/src/window_memory/live.rs); [similarity.rs:41-50](src-tauri/src/conversation/similarity.rs).
13. **Speech boundaries are pause-based (0.6 s)**, so one multi-part question can be split into turns and two questions in one breath can be one turn — [speechmatics.rs:89](src-tauri/src/stt/speechmatics.rs). Turn boundaries don't line up with question boundaries.
14. **Unused / stale scaffolding** that could mislead future work: `interview_exchanges` never written, `topic_summaries` refreshed from it (always empty) and only on typed answers, RAG docs claiming wiring, "prefetch"/"speculative" comments — see §3, §4, §6.

### Inferences
- The fastest visible fix for complaint (1) is a question label generated from the whole window (multi-question aware). The bigger "true copilot" fix is a small structured understanding step (per-utterance or per-window labels: question / multi-part / follow-up-of-X / statement / small talk / self) whose result both labels the screen and shapes the answer request — none of which exists yet.

### Gaps
- Not verified against real call recordings; all impact statements are inferences from code.

---

## 8. Reference implementations in `references/`

### Takeaway
`references/Repo1-Natively` and `references/Repo2-OpenCluely` **do not exist** in this checkout. `references/` holds one Electron app called "Assistant" (local-first, Ollama + faster-whisper). It also has no utterance classifier. Its Ctrl+Enter "question" is a **time slice since the last press (other party only) plus up to 180 s of earlier raw transcript as background**, and it explicitly tells the model to continue a thread for follow-ups.

### Cited Findings
- `ls references` shows `README.md, SPEC.md, src/, python/ …` and no `Repo1-Natively` / `Repo2-OpenCluely` directories; README: "Local-first, multi-window AI meeting & work assistant ... Ollama for inference, faster-whisper for transcription" — [references/README.md:1-6](references/README.md). Aside's own log already notes the two named repos are missing — [docs/decisions.md:195, 286](docs/decisions.md).
- **Question slice.** `takeQuestionSlice`: the question = segments since the breakpoint; when system audio exists only the other party counts (user's mic ignored); lines that echo a recent AI answer are dropped; if nothing new, falls back to their last 2 lines; background = up to `BACKGROUND_SECONDS = 180` before the breakpoint, trimmed to `BACKGROUND_MAX_TOKENS = 1000`; the breakpoint then moves to now — [references/src/main/services/TranscriptStore.ts:18-20, 77-115](references/src/main/services/TranscriptStore.ts).
- **Answer prompt.** `composeTranscriptAnswerPrompt`: "Earlier in the meeting, for context: …" + "Since then, this was just said — answer it now: …" + "If it is a follow-up to an earlier point, continue the thread and add new, specific detail — do NOT repeat an earlier answer. Do not restate the question…" — [references/src/main/services/ChatService.ts:47-65](references/src/main/services/ChatService.ts). It folds both into the turn right before the answer "not above the whole chat history, where the model would drift to an earlier question" — [ChatService.ts:47-53](references/src/main/services/ChatService.ts).
- **Continuation.** A separate "continue your previous answer" prompt that restates the unchanged question — [ChatService.ts:67-85](references/src/main/services/ChatService.ts). Chat history (earlier answers) stays in the message list, with token budgeting — [ChatService.ts:443-492](references/src/main/services/ChatService.ts).
- **Suggestions.** Every ~15 s a small model reads the last ~90 s and proposes ≤2 questions the user may want to ask the copilot (strict JSON, skipped if the main model is busy) — [references/src/main/services/SuggestionEngine.ts:9-25, 79-108](references/src/main/services/SuggestionEngine.ts).
- **Modes / tone.** Per-mode system prompts and an optional casual-tone directive — [references/src/shared/casual.ts](references/src/shared/casual.ts); adaptive answer length classified by the model with a 1-liner band — [references/src/shared/answerLength.ts:1-35](references/src/shared/answerLength.ts).

### Inferences
- The reference does two things Aside lacks that matter for the owner's goals: (a) it keeps **raw** recent background (180 s) instead of only 1-2 sentence summaries, so follow-ups have real context; (b) it keeps **previous answers** in history and has an explicit continuation path. It does **not** solve multi-question labelling or utterance classification either ("Do not restate the question").
- Aside's window design (both lanes, everything since last press) is already close to the reference's breakpoint slice; Aside includes the user's own mic, which the reference deliberately excludes when system audio is present.

### Gaps
- Natively and OpenCluely could not be compared because they are not in the checkout; any comparison with them must come from other research.
