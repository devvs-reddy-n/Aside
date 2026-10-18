# Competitor products: how live interview/meeting copilots detect, pick, and answer questions

Research date: 2026-10-17. Star counts and licences are as returned by the GitHub API on that date.

How sure each claim is, marked inline:
- **[CODE]**: read directly in the repository source on 2026-10-17.
- **[PROMPT-LEAK]**: from a leaked system prompt; whether it is genuine cannot be checked.
- **[VENDOR]**: the company's own marketing or docs.
- **[REVIEW]**: third-party reviews or user reports, often from competitors and usually small samples.

Note for the report writer: the local folders `references/Repo1-Natively` and `references/Repo2-OpenCluely` that CLAUDE.md names do **not** exist in this checkout. `references/` holds a different Electron project called "Assistant". All Natively and OpenCluely findings below come from the public GitHub repos. Code quotes are kept to short phrases or paraphrased, especially for Natively, whose licence restricts reuse (see the Natively section).

## Open-source projects: the code that decides what to answer, the prompts used, and licences

### Takeaway
The open-source projects range from "send every pause to the model" (Pluely, OpenCluely, Cheating Daddy) to Natively, the most advanced. Natively uses a staged pipeline:
1. Wait for the other person to stop talking.
2. Run a cheap local filter.
3. Ask one small, fast LLM "judge" whether this is a complete ask aimed at the user. The judge returns JSON with an extracted `question_text`.
4. Only then generate the answer.

Natively also keeps separate deterministic resolvers for follow-ups and a "question ledger" for several open questions; the ledger is still in shadow mode. Every other project hands the decision to the main answering prompt ("if it's small talk, say you're listening"), or makes the user press a key or click a question.

### Cited Findings

**Natively** (Natively-AI-assistant/natively-cluely-ai-assistant, about 3,004 stars, Electron with a Rust audio layer)
- **Licence:** a custom "Natively Personal Use Source License v1.0", effective 1 June 2026. It calls itself "source-available, not open source".
  - Allowed: viewing, studying and forking for personal, educational, research or non-commercial use.
  - Forbidden without written permission: any commercial use. The listed examples include a desktop app, a startup, paid interview assistance and paid meeting assistance.
  - Any derivative work must carry the same licence.
  - The licence's definition of the "Software" explicitly includes prompts and configuration files, and its "Derivative Work" covers ports and adaptations.
  - Consequence for Aside: copying prompt text, or porting code line by line, is off-limits. Learning the ideas is allowed. — [LICENSE](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/LICENSE) [CODE]
- **(e) Manual vs automatic:** Manual is the default. A "What to answer" hotkey or button is the primary trigger, and Auto Answer (Beta) is off by default, enabled in Settings > General. — [README](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/README.md); [autoAnswerGate.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswerGate.ts) [CODE]
- **Auto-answer guards:** the gate checks, in order:
  - the feature is on;
  - a meeting is active;
  - the meeting has not been stopped and restarted while the timer was pending (a "generation" counter);
  - the last interviewer turn is non-empty and *final*, never a partial;
  - it is not exactly the question already answered;
  - the engine is not busy or cooling down.
  - The code comments explain why the duplicate check exists: without it, an interviewer who asks and then thinks aloud would trigger a fresh answer every 3 s. — [autoAnswerGate.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswerGate.ts) [CODE]
- **(a) Detection pipeline (SimpleAutoAnswer, decided 2026-08-25):** the authors call it "legacy trigger, judge brain".
  - The interviewer's speech must stop for a stability window of 900 ms. It shortens to 350 ms when the speech-to-text provider sends an end-of-speech signal.
  - A free local filter drops duplicates, pure acknowledgements ("yeah", "mm-hm", "got it", "makes sense") and anything under 4 new words without a "?".
  - Then exactly ONE judge call is made per stoppage, never one per transcript chunk.
  - If new interviewer speech arrives while the judge call is in flight, the result is thrown away and the next stoppage is judged with more context.
  - If the judge is unavailable, the fallback answers only speech ending in "?". On providers that don't punctuate, it answers speech starting with a question word (how/what/why/can/tell me/walk me…).
  - The authors say they arrived here after six live rounds in which more complex machinery (quiet windows, re-judging revisions, act heuristics) "ate" real questions. — [SimpleAutoAnswer.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/SimpleAutoAnswer.ts) [CODE]
- **Early judging to cut latency:** the judge is asked after only 120 ms of quiet, while the answer is still committed at 900 ms, so the ~1.3 s judge call overlaps the wait. Before this, answers landed ~2.2 s after the interviewer stopped. Only the cheap judge (~2.2k tokens on flash-lite) runs early, never the full answer. — [SimpleAutoAnswer.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/SimpleAutoAnswer.ts) [CODE]
- **Judge design (2026-08-24):**
  - The user knowingly overrode an earlier "no cloud LLM in detection" rule, because fixed regex detection "generalizes badly": every new video needed a new pattern.
  - The heuristic detector became the prefilter and the fallback.
  - Judge deadline is 2.5 s; flash-lite measured 750–1,200 ms.
  - Context is limited to the last 8 turns, each cut to 400 characters. Tails are kept rather than heads, because "an ask lands at the END of speech".
  - Measured latency is driven by input size: ~1.0 s at ≤2k tokens, 1.58 s at 7.5k, 3.77 s at 28k on gemini-3.1-flash-lite. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **Judge output** is JSON with: `is_ask`, `directed_at_user`, `complete`, `act` (question / follow_up / coding_task / behavioral / technical / rhetorical / statement / social / incomplete), `action` (answer / silent), `answerability` (0 to 1), and `question_text`.
  - `question_text` must be quoted verbatim from the speech and be the WHOLE ask, so a task stated in several parts keeps all of them, not only the last part.
  - The parser checks that at least 65% of the extracted question's words appear in the speech. Otherwise it treats the extraction as invented and uses the raw speech. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **(c) Statements vs questions: the judge's rules, paraphrased** from live meetings. NOT asks:
  - rule explanations, demos, storytelling and thinking aloud, even with question words;
  - a question the same voice answers itself;
  - permission or scoping ("you can skip auth", "take your time");
  - logistics ("share your screen", "20 minutes left");
  - deferrals ("we'll come back to that");
  - the interviewer paraphrasing the user ("so you're saying…");
  - comprehension tags ("…, right?");
  - first-person narration of what the speaker is handing over;
  - statements that expect only acknowledgement.

  The judge also follows these rules:
  - A task stated as a statement IS an ask ("your task is to recreate this game in React"). Questions do not need a "?".
  - A merged reply from *another participant* never closes a question.
  - Questions the user asked (permission or clarification, like "Can I code in Python?") must not be answered.
  - Asks addressed to someone else by name are silent. The user's name comes from the résumé or calendar, and the prompt also lists speech-to-text misspellings of it.
  - "WHEN IN DOUBT, ANSWER". The middle "offer card" option was removed because asking permission costs the user a keystroke at the worst moment. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **(b) Multi-part questions, inside the judge:**
  - A speaker who announces a structure ("three operations", "two parts", "first… second…") and hasn't delivered it all is marked *incomplete*: wait rather than answer half.
  - Trailing on a conjunction or preposition, or "your task is—", also counts as incomplete.
  - Prompt order matters, as measured: the rules and JSON schema must come LAST, after the untrusted transcript. A cache-friendly layout with instructions first was tried and reverted: caching never engaged, and merged-turn asks regressed from 3/3 to 0/3.
  - The "already answered" ask goes in the trailing block too. With it in the preamble, the model fired on five elaborations of a task it had just answered.
  - Restatements of an answered ask score at most 0.2. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **Answer threshold and judge scores:** the answer floor is 0.30. Logged judge scores are effectively bucketed into a few values: 0 (×132), 0.1 (×69), 0.4 (×5), 0.8 (×2), 0.9 (×19), 1.0 (×13). Fine-tuning the threshold therefore barely matters. The one questionable answer came from the 0.4 band ("I recommend maybe sharing your screen"). — [SimpleAutoAnswer.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/SimpleAutoAnswer.ts) [CODE]
- **The user's own microphone has no vote** (decided 2026-09-03). The user's speech never cancels a streaming answer, because people start answering the moment a question lands. The mic is still transcribed so the judge sees both sides. — [SimpleAutoAnswer.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/SimpleAutoAnswer.ts) [CODE]
- **(b) The "Question ledger"** (2026-08-18, a deterministic store, currently SHADOW-ONLY: it records but does not drive answers). Its header names the problem: the live path is stateless, "latest eligible interviewer turn wins" over a 180 s window, so "earlier unanswered questions silently vanish, compound questions collapse to one blob". The ledger records:
  - every detected ask;
  - its relation to earlier asks (refinement, correction, topic shift);
  - whether the candidate has answered it, using word-overlap linking.

  Linking questions by meaning was deferred to a later embedding or small-model phase. — [questionLedger.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/questionLedger.ts) [CODE]
- **Deterministic question extractor (no LLM, under 1 ms):**
  - Pulls the latest meaningful interviewer question from the last ~180 s, removing greetings and filler.
  - Records perspective, so "tell me about your projects" is answered as the candidate, not as the AI.
  - Flags follow-ups and their target.
  - Has a "split constraint" rule: a following turn like "assume the input is sorted" within 30 s is joined to the previous ask instead of replacing it. — [transcriptQuestionExtractor.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/transcriptQuestionExtractor.ts) [CODE]
- **Fixed priority order** for what to answer: explicit typed question > user-selected transcript segment > latest stable interviewer question > a follow-up resolved against conversation state. The authors write that a transcript extractor overriding what the user typed is a bug. — [question-resolver.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/context-intelligence/question/question-resolver.ts) [CODE]
- **(d) Follow-ups:**
  - FollowUpResolver turns bare fragments ("And SQL?", "Why?", "What about complexity?", "with code?") into full questions that take on the previous question's subject and answer type.
  - If there is no prior context, it emits a clarification instead of letting the model ramble or dump the profile.
  - sessionFollowupResolver adds long-range memory, so "that project" at minute 62 resolves to a project mentioned at minute 1, with decay over time and correction handling.
  - It has a guard for "How big is your team, and what is your role on it?", where "it" refers to something named earlier in the same question. — [FollowUpResolver.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/FollowUpResolver.ts); [sessionFollowupResolver.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/sessionFollowupResolver.ts) [CODE]
- **Manual trigger is never throttled:** a bug fix notes that background prefetching kept refreshing the cooldown timer, so the user's manual hotkey silently stopped working after a few questions. Explicit user actions now bypass the cooldown. — [triggerGate.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/triggerGate.ts) [CODE]
- **(g) Answer prompts:**
  - The "What to answer" prompt makes the model speak AS the user in first person, 2–4 natural sentences, with implicit STAR (Situation, Task, Action, Result) for behavioral questions.
  - It uses only facts from the résumé or profile, and says impact was qualitative rather than inventing numbers.
  - Length tiers: a spoken short answer under ~100 words by default; "spoken full" of 100–180 words for multi-part questions, trade-offs or behavioral stories; "structured full" for code, system design and recaps. — [prompts.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/prompts.ts) [CODE]
- **Cleaning up answers:** in a 1,000-turn campaign, about one answer in 300 opened with a canned "sorry, could you clarify…" line followed by real content. Natively strips such openers rather than regenerating. — [cannedOpener.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/cannedOpener.ts) [CODE]
- **(f) Personalization:**
  - Résumé + job description context, automated company research "dossiers", job-description requirement coverage scoring and a salary negotiation copilot are Pro-only.
  - Local RAG over past meetings (SQLite + sqlite-vec; RAG means looking up relevant passages and adding them to the prompt) is free.
  - A browser companion pulls the current page (for example a job description) into the next answer.
  - A separate faster "Background Model" handles Auto Answer decisions. — [README](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/README.md) [VENDOR]
- **Latency benchmark** in the repo (360-turn dev set, 9 modes): time to first text p50 / p95 ≈ 854 / 1,337 ms; total time p50 / p95 ≈ 1,555 / 2,753 ms on the improved run. The README markets "<500ms latency", which refers to audio capture, not answers. — [existing-runs.md](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/benchmarks/natively-answer-quality/astra/out/latency/existing-runs.md) [CODE]; [README](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/README.md) [VENDOR]
- **Default models:** gemini-3.1-flash-lite (the .env example's DEFAULT_MODEL). Groq is recommended for speed. — [README](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/README.md) [VENDOR]

**OpenCluely** (TechyCSR/OpenCluely, about 1,104 stars, Electron, Gemini + Whisper)
- **Licence conflict:** the GitHub API reports Apache-2.0, but the README badge says MIT. Either is permissive. — [repo](https://github.com/TechyCSR/OpenCluely); [README](https://github.com/TechyCSR/OpenCluely/blob/main/README.md) [CODE]
- **(a)/(b) Detection:** there is no question detection at all. Each voice-detector transcript fragment is added to a buffer. An 800 ms timer collects the fragments so one question makes one LLM call. If a call is already running, new speech waits and is sent together when it finishes. The result: several back-to-back questions are combined into one blob. Manual-capture mode skips the timer. — [main.js](https://github.com/TechyCSR/OpenCluely/blob/main/main.js) (handleTranscriptionFragment / dispatchCoalescedUtterance) [CODE]
- **(c) Statements:** handled only inside the answer prompt (the "Intelligent Transcription Response System"). If speech is casual or off-topic for the selected skill (DSA, programming…), the model replies with a stock "I'm listening, ask your … question" line. Otherwise it gives a "comprehensive, detailed response" with bullets. — [llm.service.js](https://github.com/TechyCSR/OpenCluely/blob/main/src/services/llm.service.js) [CODE]
- **(d) History:** sends the last 10–15 conversation turns as chat history with each request. — [llm.service.js](https://github.com/TechyCSR/OpenCluely/blob/main/src/services/llm.service.js) [CODE]

**Cheating Daddy** (sohzm/cheating-daddy, about 5,631 stars, GPL-3.0)
- **(a)/(e) Detection:** the model itself decides when to answer. It streams audio to the Gemini Live API (a real-time audio API) with speaker labelling on (minimum 2 speakers, labelled Interviewer/Candidate) and Gemini's `proactiveAudio` option, which lets the model choose not to respond. The server decides when a turn is complete. With a Groq key set, the final transcript of each turn goes to Groq as text instead, with a 20-message history. — [gemini.js](https://github.com/sohzm/cheating-daddy/blob/master/src/utils/gemini.js) [CODE]
- **(g)/(f) Prompts:**
  - Built from a profile (interview / sales / meeting / exam…) plus a free-text "User-provided context" block where the user pastes résumé and job description.
  - Answers are 1–3 sentences in "exact words to say" form, with example answers inside the prompt.
  - Google Search is used for recent events or company-specific questions.
  - The exam profile repeats the question text before the answer so the user can check it was read correctly. — [prompts.js](https://github.com/sohzm/cheating-daddy/blob/master/src/utils/prompts.js) [CODE]
- **(d) Reconnects:** after a dropped connection, it re-sends the last 20 interviewer/answer pairs as a "Session reconnected" context message. — [gemini.js](https://github.com/sohzm/cheating-daddy/blob/master/src/utils/gemini.js) [CODE]

**Pickle Glass** (pickle-com/glass)
- The GitHub API returns 404 for the original repo on 2026-10-17. The code was read from a GPL-3.0 fork, [mickaelleclercq/glass](https://github.com/mickaelleclercq/glass). [CODE]
- **(e) Two separate paths:**
  - "Listen" runs a structured analysis automatically every 5 transcript turns: summary, key topic, explanation and 3 suggested questions. It builds on the previous analysis and uses the last 30 turns.
  - "Ask" is user-triggered and sends the transcript plus a screenshot. — [summaryService.js](https://github.com/mickaelleclercq/glass/blob/main/src/features/listen/summary/summaryService.js) [CODE]
- **(a)/(c) Ask prompt:** a decision order:
  1. Answer a recent question, even if lines follow it.
  2. Otherwise define the last proper noun.
  3. Otherwise solve a clear problem on screen.
  4. Otherwise "Not sure what you need help with" plus a 1–2 bullet recap.

  The format is a headline of 6 words or fewer, then 1–2 bullets. Behavioral answers must use only real user history, otherwise they start with "User context unavailable". — [promptTemplates.js](https://github.com/mickaelleclercq/glass/blob/main/src/features/common/prompts/promptTemplates.js) [CODE]

**Pluely** (iamsrikanthnani/pluely, about 2,746 stars, GPL-3.0; a **Tauri/Rust** app like Aside)
- **(a) Detection:** the system-audio voice detector waits for about 1.0 s of silence (45 silent chunks, peak threshold 0.035). Each detected utterance is transcribed and sent straight to the AI with the system prompt. There is no question filter. — [useSystemAudio.ts](https://github.com/iamsrikanthnani/pluely/blob/master/src/hooks/useSystemAudio.ts) [CODE]

**Cue** (Blueturboguy07/cue, about 1,456 stars, GPL-3.0)
- **(e)/UX: "Answer This" mode.** The user picks one specific transcript question. The prompt says to focus ONLY on that question and ignore other conversation. Only that question is sent, not the transcript history.
- **Answer types:** behavioral answers use STAR from the user's real stories (Situation 1 sentence → Task 1 → Action 2–3 → Result 1, with a metric). "Any questions for us?" answers offer 2–3 of the user's prepared questions. Compensation answers state the user's salary target. — [prompts.js](https://github.com/Blueturboguy07/cue/blob/main/src/prompts.js) [CODE]
- Also has separate résumé-context, interview-context and meeting-memory modules (file names only, not read). — [repo tree](https://github.com/Blueturboguy07/cue) [CODE]

**MeetingCopilot** (JWM0203/MeetingCopilot, about 327 stars, Apache-2.0, Chinese/English)
- **(a) Continuous-mode gate:** a deliberately loose regex check. Speech counts as a question if it ends in ?/？, contains a question word (Chinese or English: what/how/why/can you/tell me/explain), or is 40 characters or longer ("long utterances often carry an implicit ask").
- **Regex classifier:** sorts questions into smalltalk / behavioral / technical. It only adds a one-line "hint" the model may override. — [textHeuristics.ts](https://github.com/JWM0203/MeetingCopilot/blob/main/shared/textHeuristics.ts) [CODE]
- **(g)/(f) Prompt layout, built for caching:**
  - A byte-stable prefix (persona + résumé + job description + language), so DeepSeek's prefix cache can reuse it.
  - A slow-changing "interview memo", updated every few question/answer rounds by a separate LLM call.
  - A fast part: recent turns + this question + hint.
  - Modes: manual "answer THIS sentence", continuous, free-ask and translate. — [prompts.ts](https://github.com/JWM0203/MeetingCopilot/blob/main/electron/llm/prompts.ts) [CODE]

**meetingly-ai** (iskandaryv/meetingly-ai, about 289 stars, Apache-2.0)
- **Suggestions rail:** after a 2 s pause (at most once per 20 s, and only after 80–120 new characters), an LLM suggests up to 4 clickable questions. If the other side just asked the user something, suggestion #1 is that question restated, with "it/that" spelled out.
- **Running notes:** the model also returns a 6-word "topic" and a 60-word running "context" of lasting facts that carries across runs. Clicking a suggestion produces the answer. — [suggestions.ts](https://github.com/iskandaryv/meetingly-ai/blob/main/app/electron/services/suggestions.ts) [CODE]

**Meetily** (Zackriya-Solutions/meetily, about 31,860 stars, MIT)
- A local transcription and summary app (Whisper/Parakeet speech-to-text, Ollama/Claude/Groq for summaries). No live question answering was found in the README. — [README](https://github.com/Zackriya-Solutions/meetily) [VENDOR]

**free-cluely** (Prat011/free-cluely, about 1,689 stars, Apache-2.0)
- The file tree (ScreenshotQueue, ProcessingHelper, Solutions pages) suggests a manual, screenshot-driven flow. The source was not read. — [repo](https://github.com/Prat011/free-cluely) [CODE, file names only]

**Cluely** (commercial; leaked prompt) [PROMPT-LEAK]
- The leaked "Enterprise" system prompt in [x1xhlol/system-prompts-and-models-of-ai-tools](https://github.com/x1xhlol/system-prompts-and-models-of-ai-tools/blob/main/Cluely/Enterprise%20Prompt.txt) tells the model to help "at the current moment" (the end of the transcript).
- Order of actions: answer a question at the end > define a proper noun in the last 10–15 words > suggest 1–3 follow-up questions > handle an objection (labelled "Objection: [name]") > solve the screen > passive "Not sure what you need help with right now".
- It says to treat speech as a question at "50%+" confidence, and to focus on intent rather than a "?". Incomplete questions ("so the performance…") and implied ones ("walk me through Z") count.
- Speakers are labelled "me" / "them", with a warning that labels are often wrong.
- Answer format: a headline of 6 words or fewer, then 1–2 bullets of 15 words or fewer.
- Glass's prompt is clearly based on this same structure.

### Inferences
- The most transferable pattern is Natively's two-model split: a tiny, fast, structured "is this an ask, for me, finished?" judge, with the expensive answer model run only after a yes. The judge should also return a verbatim, grounded `question_text`; that solves "what exactly to show and answer". This fits a Rust backend well: the prefilter and timers are pure functions, and the judge is one HTTP call with a deadline and a fallback.
- Natively's own history is a warning against over-engineering detection: six rounds of complex state machines lost real questions, and the simple design won ("trigger on stop, judge once, cancel if speech resumes").
- No project, Natively included, yet *lists* several distinct open questions in the live path. Natively's ledger exists but is shadow-only. The common behaviors are:
  - Natively's judge keeps the WHOLE multi-part ask as one question;
  - OpenCluely merges everything said since the last answer;
  - Glass and Cluely answer only the latest question.

  Showing every open question would set Aside apart.
- Licensing for Aside: Natively's prompt wording and code cannot be reused (its licence names prompts and ports). GPL projects (Cheating Daddy, Pluely, Cue, Glass) would force GPL on derived code. Apache/MIT projects (OpenCluely, MeetingCopilot, meetingly-ai, Meetily, free-cluely) are safer if code reuse is ever wanted. Ideas and patterns can be adapted from all of them.

### Gaps
- Interview Coder's source (interview-coder repos) was not inspected. A 2026 roundup describes it as manual and screenshot-based.
- The original pickle-com/glass repo is gone or private; the fork may differ from the last official version.
- Whether Natively's ledger has been promoted from shadow mode to live could not be confirmed beyond the file header.
- The leaked Cluely prompt is unverified and dated (circulated in 2025). Cluely's current detection logic is unknown.

## Commercial products: question detection accuracy, multiple questions, false triggers, latency, personalization

### Takeaway
Interview copilots split into two camps:
- **Automatic** detection using two audio channels (Final Round AI, Verve, Sensei, LockedIn): the other person's audio and the user's mic are kept separate, which tells the tool who spoke.
- **Manual** trigger (Parakeet in some reviews, Interview Coder, Cluely).

Public evidence on accuracy is almost entirely user complaints and vendor claims. The recurring complaints:
- a question at the end of a long statement is missed (Parakeet);
- misread questions, or several answers at once (Final Round);
- generic answers;
- lag of 4–10 s that reads as hesitation (LockedIn, Cluely).

Meeting assistants (Otter, Zoom, Teams, Granola) do not auto-answer at all. The user asks them questions about the transcript.

### Cited Findings
- **Final Round AI:** reviewers describe it as a teleprompter that transcribes the interviewer's questions and suggests answers. Trustpilot complaints include the AI misreading questions, producing several answers at once, generic replies, glitchiness, and live answers scrolling too fast to read. Praise covers handling of accents and help structuring answers. — [Trustpilot p5](https://www.trustpilot.com/review/finalroundai.com?page=5); [Trustpilot AU p4](https://au.trustpilot.com/review/finalroundai.com?page=4); [Dataconomy](https://dataconomy.com/tools/final-round/) [REVIEW]
- **Final Round AI (roundup):** described as auto-detecting with dual-channel audio. Supports document upload beyond the résumé but has no prepared Q&A. Pricing complaint: $149 for 5 sessions. — [Vizologi, 2026-07-14](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW, method not disclosed]
- **Parakeet AI:**
  - Markets "detects questions live".
  - Its public feedback board has user reports titled "The ai is unable to detect questions": the report says a question at the end of a longer statement is missed.
  - Other reports: an Autogenerate toggle still showing a manual "Answer Question" button, only the first answer auto-generating, answers about "a different context", and an "Answer" button becoming unresponsive.
  - The board itself requires login; titles and summaries come from search snippets. — [featurebase list](https://parakeetai.featurebase.app/p/the-ai-is-unable-to-detect-questions); [autogenerate issue](https://parakeetai.featurebase.app/p/issue-with-autogenerate-toggle-manual-button-still-appearing); [irrelevant context](https://parakeetai.featurebase.app/p/issue-with-parakeetai-providing-irrelevant-context-answers) [REVIEW]
- **Parakeet AI (roundup):** described as manual, single audio channel, unable to reliably separate voices, and generic (not drawing on the candidate's background). — [Vizologi](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW]
- **Parakeet AI (directory):** uses an uploaded CV in interview mode, and models from OpenAI, Anthropic and Google. — [EveryDev](https://www.everydev.ai/tools/parakeetai) [VENDOR/directory]
- **LockedIn AI:**
  - The official page describes a "dual layer": an AI Copilot plus an AI Coach running in parallel. Users upload a résumé and job description for personalization. — [lockedinai.com/ai-copilot](https://www.lockedinai.com/ai-copilot) [VENDOR]
  - Directory listings claim "116 ms response time" and "fine-tuned GPT-4o". These are unverified. — [directory](https://www.techjockey.com/detail/lockedin-ai) [VENDOR]
  - A review (2026) reports suggestions about 4–5 s after the question ends, "the most consistently cited technical limitation". Other complaints: STAR-structured but GPT-sounding answers, too generic on ambiguous follow-ups, and a cluttered interface. The reviewer is itself a competing product. — [CloudColleague](https://cloudcolleague.com/blogs/interview-tips/lockedin-ai-review/) [REVIEW]
- **Verve AI:** claims question-type detection under 1.5 s (its own blog). The roundup measured 1–2 s, auto-detection with dual-channel audio, and "Knowledge Banks" plus prepared Q&A pairs. — [Verve blog](https://www.vervecopilot.com/hot-blogs/ai-interview-copilot-zoom) [VENDOR]; [Vizologi](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW]
- **Sensei AI:**
  - Directories describe drafting answers from the résumé, personal stories and the target job description, plus auto-detection of live questions and a personal story bank. — [theaireport](https://www.theaireport.ai/tooldatabase/sensei-ai); [LoopCV review 3.6/5](https://www.loopcv.pro/directory/sensei-ai/) [VENDOR/REVIEW]
  - The roundup found it the fastest tested (under 1 s, auto, dual-channel). It has "Story Studio" (pre-written STAR stories), but no custom prompts or prepared Q&A. — [Vizologi](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW]
- **Cluely:**
  - A Medium user saw 5–10 s delays for mid-call requests.
  - Users reported hallucinations: a claimed Portuguese fluency the user never had; a meeting's key decision stated wrongly.
  - Praised for follow-up emails, action items, and objection handling on sales calls.
  - Several of these sources are competitors. — [Medium](https://medium.com/@wenhaooyang/cluely-i-tried-the-ai-that-cheats-for-you-in-meetings-a53a89c01d56); [Techjockey](https://www.techjockey.com/reviews/cluely); [Bluedot](https://www.bluedothq.com/blog/cluely-alternatives) [REVIEW]
  - The roundup lists Cluely as manual and single-channel. — [Vizologi](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW]
- **Otter.ai:** the meeting chatbot (launched June 2023) lets participants ask during or after the meeting ("What did I miss?", "What did we say about the budget?"). Answers come from the transcript. — [TechCrunch 2023-06-21](https://techcrunch.com/2023/06/21/otter-is-introducing-a-meeting-oriented-ai-chatbot) [VENDOR via press]
- **Granola:** per-meeting chat works before, during and after a meeting. Home/folder chat defaults to summaries of the last 40 meetings, and users can pick specific meetings as context. "Recipes" are reusable prompts grouped as before / during / after the meeting. — [Granola help](https://help.granola.ai/article/chatting-with-your-meetings); [TechCrunch](https://techcrunch.com/?p=3052227) [VENDOR]
- **Zoom AI Companion:** preset in-meeting questions ("Catch me up", "Was my name mentioned?", "What are the action items?") plus custom prompts, all answered from the live transcript. The host controls who can ask. It cannot tell apart several people speaking from the same room. — [Zoom tips](https://preview.zoom.com/en/blog/ai-companion-tips-and-tricks/); [UAF](https://www.uaf.edu/nooktech/services/video-conferencing/zoom/ai-companion.php) [VENDOR/institutional]
- **Microsoft Teams Copilot:** prompts include "What questions were asked, answered, and unresolved?" Copilot says it needs more information when there isn't enough transcribed speech. Live transcription is needed to prompt about spoken content. — [Microsoft Support](https://support.microsoft.com/en-us/office/use-copilot-in-microsoft-teams-meetings-0bf9dd3c-96f7-44e2-8bb8-790bedf066b1); [Microsoft Learn](https://learn.microsoft.com/en-us/training/modules/ask-analyze-content-with-microsoft-copilot-microsoft-365/6-chat-copilot-about-meetings-messages-teams-outlook) [VENDOR]

### Inferences
- **Latency bar:** reviewers treat about 1–2 s as acceptable and 4–5 s or more as damaging, because it reads as hesitation. The best open-source evidence (Natively: ~0.85 s to first text p50 once triggered, plus a ~0.9 s stop window) suggests about 2 s from end of speech to visible text is achievable.
- **Dual-channel audio** (system audio vs mic) is the main thing reviewers credit for reliable automatic detection, because it reveals who spoke. Aside already captures both channels.
- **"Question at the end of a long statement"** is the most specific named failure (Parakeet). It is exactly what Cluely's "focus on the end of the transcript" and Natively's "keep tails, not heads" rules target.
- **Teams' asked / answered / unresolved framing** is the closest commercial version of an open-questions list. It is offered as an on-demand prompt, not a live display.

### Gaps
- No independent, systematic accuracy benchmark exists for any commercial copilot. The only "testing" found (Vizologi) does not disclose its method.
- No vendor documentation was found explaining how Final Round, LockedIn, Verve, Sensei, Parakeet, Beyz or Interviews Chat handle several questions in a row or multi-part questions. Beyz and Interviews Chat were not researched (time).
- Reddit threads specifically about misfires were not surfaced by search.
- Read.ai and Fireflies live-assist features were not researched.
- Directory latency claims (LockedIn's "116 ms", Sensei's "sub-1 s") are unverified.

## UX patterns for showing "the question", and answer structures

### Takeaway
There are four ways to show the question:
- **Detected-question card / echo:** show the extracted question above the answer. Natively's `question_text`, Cheating Daddy's exam mode and Cue's "Answer This" bubble do this.
- **Clickable question list:** meetingly-ai's suggestions rail, where #1 is the interviewer's question restated; Glass's "Suggested Questions".
- **Pick a transcript line to answer:** Cue "Answer This", MeetingCopilot "answer THIS sentence", Natively's "user-selected segment" priority.
- **No question shown at all:** most auto-answer tools.

Answers converge on a short first line, then brief detail:
- Cluely/Glass: a headline of 6 words or fewer, then 1–2 bullets.
- Natively/Cheating Daddy: 2–4 spoken sentences in first person.
- STAR for behavioral questions, from the user's real stories.
- A fixed section layout for coding.

### Cited Findings
- **Natively's judge** returns `question_text` verbatim from the speech and grounded in it. This is what lets the app display and answer the exact ask, including all parts of a multi-part task. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **Natively removed its "offer card"** (a prompt asking the user to press Tab to get an answer) on 2026-08-25. The stated reason: asking permission costs a keystroke and a decision at the moment the user is being spoken to; an answer they can ignore costs nothing. — [AutoAnswerJudge.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/intelligence/autoAnswer/AutoAnswerJudge.ts) [CODE]
- **meetingly-ai** offers up to 4 clickable questions of 14 words or fewer. Rule 1 restates the other side's question with pronouns spelled out. It never suggests coaching-style questions ("How should I explain…"). It shows a 6-word "topic" line. — [suggestions.ts](https://github.com/iskandaryv/meetingly-ai/blob/main/app/electron/services/suggestions.ts) [CODE]
- **Cue's "Answer This"** shows the question as the user bubble and answers only that question in 2–5 first-person sentences. — [prompts.js](https://github.com/Blueturboguy07/cue/blob/main/src/prompts.js) [CODE]
- **Cheating Daddy's exam profile** echoes "Question: … Answer: … Why: …" so the user can confirm the question was read correctly. — [prompts.js](https://github.com/sohzm/cheating-daddy/blob/master/src/utils/prompts.js) [CODE]
- **Cluely-style structure:** a headline answer of 6 words or fewer, 1–2 main bullets of 15 words or fewer, and sub-bullets for examples or metrics. Objections get a bold "Objection: [name]" label. At most 3 follow-up suggestions at once. — [Cluely Enterprise prompt](https://github.com/x1xhlol/system-prompts-and-models-of-ai-tools/blob/main/Cluely/Enterprise%20Prompt.txt) [PROMPT-LEAK]; [Glass promptTemplates.js](https://github.com/mickaelleclercq/glass/blob/main/src/features/common/prompts/promptTemplates.js) [CODE]
- **Glass's automatic "Listen" panel:** Summary Overview, Key Topic with 3 insights, Extended Explanation, and 3 Suggested Questions, refreshed every 5 turns. — [summaryService.js](https://github.com/mickaelleclercq/glass/blob/main/src/features/listen/summary/summaryService.js) [CODE]
- **Spoken-answer tiers (Natively):** short spoken (under ~100 words, no padding), full spoken (100–180 words, for multi-part questions or stories), structured (code, design, recaps). Coding answers use fixed headings: Approach / Technique / Code / Dry Run / Complexity / Interviewer Follow-up Points. — [prompts.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/prompts.ts) [CODE]
- **Cue's STAR budget:** Situation 1 sentence → Task 1 → Action 2–3 → Result 1 with a metric. — [prompts.js](https://github.com/Blueturboguy07/cue/blob/main/src/prompts.js) [CODE]
- **Admitting missing context:**
  - Glass/Cluely start behavioral answers with "User context unavailable" when there is no profile.
  - Natively opens with a fixed disclaimer only when no profile block exists, and otherwise forbids invented metrics.
  - Sources: [Glass](https://github.com/mickaelleclercq/glass/blob/main/src/features/common/prompts/promptTemplates.js); [Natively prompts.ts](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/electron/llm/prompts.ts) [CODE]
- **User complaint about display speed:** Final Round AI's live answers "scroll too fast to read". — [Trustpilot AU p4](https://au.trustpilot.com/review/finalroundai.com?page=4) [REVIEW]
- **Personalization surfaces seen:**
  - résumé + job description upload (LockedIn, Final Round, Sensei, Parakeet, Natively Pro);
  - a story bank of pre-written STAR stories (Sensei "Story Studio");
  - prepared Q&A pairs and knowledge banks (Verve);
  - company research dossiers (Natively Pro);
  - a pasted free-text context block (Cheating Daddy);
  - past-meeting memory or RAG (Natively, Granola's last 40 meetings);
  - a running interview memo (MeetingCopilot);
  - a running "context" of lasting facts (meetingly-ai).
  - Sources: [Vizologi](https://vizologi.com/7-best-ai-interview-assistants-for-coding-2026-update/) [REVIEW]; [Natively README](https://github.com/Natively-AI-assistant/natively-cluely-ai-assistant/blob/main/README.md) [VENDOR]; [MeetingCopilot prompts.ts](https://github.com/JWM0203/MeetingCopilot/blob/main/electron/llm/prompts.ts) [CODE]; [Granola help](https://help.granola.ai/article/chatting-with-your-meetings) [VENDOR]

### Inferences
- A good fit for Aside combines the best-evidenced pieces:
  - show the detected question (verbatim, grounded) above the answer, so the user can catch a misheard or wrong pick at a glance;
  - when several questions are open, list them (a ledger-style view) rather than silently answering only the last;
  - start the answer with a one-line headline or "say this first" sentence, then 2–4 sentences or bullets;
  - use STAR from the user's own stories for behavioral questions;
  - let a click on any transcript line or listed question re-target the answer (Natively's priority order: typed > selected > latest > follow-up).
- Answers should stay short and stable on screen. Fast-scrolling output is a named complaint, and the 6-word headline pattern exists so the user can read it in under a second.

### Gaps
- No usability study compares "show the detected question" against "answer only".
- No commercial product was found publicly documenting an editable detected-question field. Whether Final Round, LockedIn or Verve show a question card could not be verified from docs (screenshots and demos not reviewed).
