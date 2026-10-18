# Utterance understanding for a live interview/meeting copilot: dialogue acts, question detection, multi-question consolidation, follow-up tracking

Scope: from a streaming two-speaker transcript (Me = mic, Others = system audio), work out what each stretch of speech *is* and what Aside should *do* with it. Researched 2026-10-17. About 18 tool calls: web search/fetch plus a look at the local `references/` folder. Paper numbers are cited. Design recommendations are marked as inferences.

Note on the local references: `references/` currently holds one Electron project (README calls it "Assistant": local-first, Ollama + faster-whisper). It is not two folders named `Repo1-Natively` and `Repo2-OpenCluely`. Two patterns in it are relevant here:
1. It answers "everything said since the last real question" (a "transcript breakpoint"), and has a "continue previous answer, same question" action that does not move the breakpoint (`src/shared/ipc.ts`, `src/renderer/src/windows/overlay/LengthBar.tsx`).
2. It classifies and answers in **one call**. The model's first output is a hidden tag such as `[[LEN:SHORT]]` that sizes the answer, and the backend strips it (`src/shared/answerLength.ts`).

I found no explicit question-detection classifier in it. (Local file inspection, not a web source.)

---

## 1. Dialogue act taxonomies, and LLMs vs small fine-tuned classifiers on DA / question detection

### Takeaway
On fine-grained dialogue-act (DA) tagging (40–50 classes, multiparty), zero/few-shot LLMs still trail fine-tuned encoders. Sometimes they don't even beat rule baselines. On a *small, task-shaped* label set (about 4–8 classes) with a tailored prompt, GPT-4-class models reach about 80% accuracy. So Aside should use a small copilot-specific taxonomy, not SWBD-DAMSL/ISO 24617-2 wholesale.

### Cited Findings
- Qamar, Tong & Huang, "Do LLMs Understand Dialogues? A Case Study on Dialogue Acts" (ACL 2025). In the 50-class multiparty setting, LLMs "often fail to outperform simple rule-based baselines" on three pre-tasks: Turn Management, Communicative Function Identification, Dialogue Structure Prediction. Errors on those pre-tasks correlate strongly with final DA errors. A human study shows a "significant gap" between LLM and human dialogue understanding. — [ACL Anthology 2025.acl-long.1271](https://aclanthology.org/2025.acl-long.1271/). (I couldn't extract the result tables from the PDF.)
- Fine-tuned RoBERTa-base reached DA-F1 62.48% vs few-shot GPT-4 at 51.13% (embodied-dialogue DA task, 2024). GPT-4 zero-shot "often rival[s]" its few-shot score. — [arXiv 2410.23535](https://arxiv.org/pdf/2410.23535)
- A RoBERTa labeler fine-tuned on SwDA (42 clustered labels) reached 80.4% test accuracy. — [arXiv 2403.12500](https://arxiv.org/pdf/2403.12500)
- ChatGPT (3-shot) trails fine-tuned baselines by 11–17 points on DailyDialog dialogue-act classification (2023). — [arXiv 2304.09582](https://arxiv.org/pdf/2304.09582) (per search-result summary)
- Counterpoint: on a 4-category tutoring-dialogue task with tailored prompts, GPT-4 reached 80% accuracy and weighted F1 0.81. — reported in the search summary of DA comparison results ([search context: arXiv 2410.23535 / related](https://arxiv.org/pdf/2410.23535)). Exact paper not pinned down; treat as indicative.
- "LLMs of Catan" (LREC-COLING 2024, STAC game dialogues): ChatGPT zero/few-shot "can keep up with baseline models trained from scratch for certain dialogue act types" but shows biases/limitations. — [ACL Anthology author page](https://aclanthology.org/people/andrea-martinenghi/)
- Martínek et al. (Interspeech 2022), weak supervision for question-type detection on MRDA. Zero-shot pretrained LMs with rules instead of prompts were "competitive" with a supervised LSTM trained on the full corpus. — [ISCA Archive](https://www.isca-archive.org/interspeech_2022/martnek22_interspeech.html); [ZCU dspace](https://dspace.zcu.cz/items/491d3aad-8dbd-47ea-87b0-209c0a53fee2/full)
- MIDAS (2019) is a DA scheme designed for open-domain human–machine spoken conversation, an alternative to SWBD-DAMSL. — [arXiv 1908.10023](https://arxiv.org/pdf/1908.10023)
- A public HF model, "asr-question-detection", classifies question vs statement in spoken text and was trained on interview data. — [PromptLayer model page](https://www.promptlayer.com/models/asr-question-detection)

### Inferences
- **Recommended Aside taxonomy.** Copilot actions are the target, not linguistic DAs. The classes collapse SWBD-DAMSL `qy/qw/qo/qh/ad/sd/b/fc` into what changes behaviour. Speaker attribution comes from the channel (Me vs Others), not the model, which removes the hardest "Turn Management" sub-task the ACL 2025 paper says LLMs fail at. Mapping:

  | Class (Others channel) | Covers (rough SWBD-DAMSL / ISO 24617-2 analogue) | Copilot action |
  |---|---|---|
  | `question` | yes/no, wh-, open, or-questions (qy, qw, qo, qr; ISO setQuestion/propositionalQuestion/choiceQuestion) | answer |
  | `request` | "walk me through your resume", "tell me about…", "I'd love to hear…" (ad action-directive; ISO request/instruct) | answer as if a question |
  | `multi_question` | several asks in one turn or consecutive turns | consolidate → one answer with ordered parts |
  | `follow_up` | question whose meaning depends on an earlier Q/A ("and why that?") | rewrite to self-contained form, answer, link to parent |
  | `clarification` | "do you mean X or Y?", "sorry, can you repeat?" (ISO checkQuestion) | short answer / update in-progress answer |
  | `context` | statements, info about role/company/team/problem (sd, sv; ISO inform) | store as memory, no answer |
  | `rhetorical_or_filler` | "you know what I mean?", "right?", backchannels, small talk (qh, b, fc) | ignore |
  | `Me` speech | user's own talk | update "already answered" state; never triggers an answer |

- Prefer a **small task-specific label set (≤8)** classified by an LLM with good few-shot examples. Optionally back it with a cheap local classifier later. Every source that shows LLMs doing badly uses 40–50 fine-grained classes. The source showing ~80% uses about 4.
- A fine-tuned encoder (DistilBERT/ModernBERT/SetFit) is the natural "fast first stage" if cloud latency or cost becomes a problem. I found no 2024–2026 benchmark of SetFit or ModernBERT on SwDA/MRDA (see Gaps).

### Gaps
- No found head-to-head of GPT-4o/4.1/Claude/Gemini vs fine-tuned BERT on SwDA or MRDA with exact numbers. The ACL 2025 tables weren't extractable (binary PDF).
- No found benchmarks for SetFit / ModernBERT / DistilBERT on DA classification on *unpunctuated ASR* text.
- ISO 24617-2 and MRDA class inventories weren't fetched. The analogues in the table above come from general knowledge, not a fetched source.

---

## 2. Question detection in ASR text, and end-of-turn detection ("has the interviewer finished asking?")

### Takeaway
Punctuation in ASR output can't be trusted. Lexico-syntactic cues matter most, and prosody/turn features add complementary signal. For "finished asking", modern systems combine VAD silence with a semantic/acoustic end-of-turn model. Speechmatics' own EndOfUtterance is silence-only (configurable 0–2 s, recommended 0.5–0.8 s). Aside can add its own semantic layer and use `ForceEndOfUtterance`.

### Cited Findings
**Question detection in speech**
- Boakye, Favre & Hakkani-Tür, "Any Questions? Automatic Question Detection in Meetings" (ASRU 2009, ICSI MRDA). Lexico-syntactic features are most important. Turn-related and pitch features add complementary information. — [Illinois Experts](https://experts.illinois.edu/en/publications/any-questions-automatic-question-detection-in-meetings/); [PDF](https://pageperso.lis-lab.fr/benoit.favre/papers/favre_asru2009b.pdf)
- Teacher-question detection from classroom ASR (3 engines) reached only F1 0.59. The earlier MRDA ASR system had WER 0.38, showing how ASR noise degrades question detection. — [W16-3623](https://preview.aclanthology.org/author-url/W16-3623.pdf)
- Industry patent framing: raw ASR output often has "no capitals or punctuation", with pauses the only segmentation cue, which motivates question detection that doesn't depend on punctuation. — [USPTO 12347426](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/12347426)

**Speechmatics (Aside's STT)**
- `end_of_utterance_silence_trigger`: 0–2 s, 0 disables. The countdown restarts on each word. When it expires the server sends a final `AddTranscript` and then `EndOfUtterance`. Recommended 0.5–0.8 s for voice AI and 0.8–1.2 s for dictation. It must be lower than `max_delay`. EndOfUtterance carries `start_time`/`end_time` and **no speaker info**, and is never duplicated for one silence. — [Speechmatics Turn detection docs](https://docs.speechmatics.com/speech-to-text/realtime/end-of-turn)
- `ForceEndOfUtterance` lets the client's own turn model (e.g. VAD) force an end of utterance at any time, with an optional `timestamp`. The server replies with `AddTranscript` + `EndOfUtterance`, both `forced: true`. Supported on Enhanced, Standard and Melia 1. The page mentions "semantic turn detection" (silence plus context) but gives no config. — [Speechmatics docs](https://docs.speechmatics.com/speech-to-text/realtime/end-of-turn)
- `max_delay` range 0.7–4.0 s, default 1.0 (per LiveKit's Speechmatics wrapper, not Speechmatics' own page). — [LiveKit SpeechmaticsOptions](https://docs.livekit.io/reference/agents-js/interfaces/agents.inference.stt.SpeechmaticsOptions.html)

**End-of-turn models**
- LiveKit text-based turn detector (docs): English model SmolLM2-135M, 66 MB, ~15–45 ms CPU per turn, TPR 98.8% (correctly says "finished") / TNR 87.5% (correctly says "will continue"). Multilingual model Qwen2.5-0.5B, 281 MB, ~50–160 ms, TPR ~99.3–99.4%, TNR ~80–94%. — [LiveKit turn-detector docs](https://docs.livekit.io/agents/logic/turns/turn-detector)
- One community report puts the multilingual model at ~0.7–1 s inference, far above the docs (unverified single user). — [LiveKit community](https://community.livekit.io/t/high-turn-detection-latency-issue/958)
- LiveKit Turn Detector v1.0 (blog 2026-06-17) works on **audio, not text**. A semantic branch (audio encoder → adapter → fine-tuned LM) plus an acoustic/prosody branch (encoder → RNN) are fused, and it looks only at the current user turn. On LiveKit's own eot-bench (14 languages): 9.9% false cutoff at a 300 ms budget (Deepgram Flux 12.9%, ultraVAD 27.7%) and 4.5% at 600 ms (Soniox 5.5%, Flux 9.9%). Mean latency is 543 ms at ≤5% false cutoffs and 295 ms at ≤10%. Hosted on LiveKit Cloud. There is a CPU "v1-mini" variant under the LiveKit Model License. Vendor-run benchmark. — [LiveKit blog](https://livekit.com/blog/solving-end-of-turn-detection)
- Pipecat Smart Turn v3 (Daily, 2025): audio-only end-of-turn model, ~12 ms on modern CPUs and ~60 ms on a cheap AWS instance (incl. ~3 ms preprocessing), ONNX. v3.1 improved accuracy (more human audio, better quantization). Community ports describe v3.2 as 23 languages, run once per pause on the last 8 s of audio. Open source. — [Daily blog v3](https://www.daily.co/blog/announcing-smart-turn-v3-with-cpu-inference-in-just-12ms/); [Daily blog v3.1](https://www.daily.co/blog/improved-accuracy-in-smart-turn-v3-1/); [GitHub pipecat-ai/smart-turn](https://github.com/pipecat-ai/smart-turn); [ONNX port](https://huggingface.co/soniqo/Smart-Turn-v3.2-ONNX)
- OpenAI Realtime `semantic_vad`: a semantic classifier on the words spoken estimates whether the user has finished. Low probability means it waits up to a timeout. Trailing "ummm" waits longer. `eagerness` low/medium(auto)/high has max timeouts of 8 s / 4 s / 2 s. — [OpenAI Realtime VAD guide](https://developers.openai.com/api/docs/guides/realtime-vad); [Ruby SDK reference](https://rubydoc.info/gems/openai/OpenAI/Models/Realtime/RealtimeTranscriptionSessionAudioInputTurnDetection/SemanticVad/Eagerness)
- Deepgram Flux: `eot_threshold` (default 0.7, range 0.5–0.9), optional `eager_eot_threshold` (0.3–0.9, must be ≤ eot_threshold), `eot_timeout_ms` (500–10000, default 5000). Events are StartOfTurn, EagerEndOfTurn, TurnResumed and EndOfTurn. — [Deepgram Flux configuration](https://developers.deepgram.com/docs/flux/configuration)

### Inferences
- **Speechmatics' 1.5 s silence EOU is a reasonable "hard" end-of-question signal but slow.** Interviewers often pause mid-question ("so… tell me about a time when… you had to deal with conflict"). Use a **two-threshold design**:
  - **Soft end** at about 500–700 ms of silence on the Others channel, detected locally from the partial/final stream timing or a local VAD. This starts a *speculative* classify/answer.
  - **Hard end** at the Speechmatics EndOfUtterance (~1.5 s), or `ForceEndOfUtterance` when a local semantic check is confident. This *commits*.
- Text-only semantic completeness ("does this read like a finished question/request?") can be done by the same small LLM classification call, or by a tiny local model like LiveKit's SmolLM2-135M approach (~15–45 ms CPU). Audio models (Smart Turn v3, ~12 ms CPU, ONNX) fit Aside's existing ONNX stack and capture rising/falling intonation that ASR text loses. **Smart Turn v3 via ONNX in Rust is the most implementable audio EOT for Aside.**
- Because EndOfUtterance carries no speaker info, Aside should keep separate Speechmatics sessions per channel (Me vs Others). Aside appears to do this already. Each EOU then maps to a channel.
- Don't rely on "?" in ASR text. Detect questions from wh-words/auxiliary inversion, imperative request verbs ("tell me", "walk me through", "describe", "explain", "talk about", "give me an example", "I'd love to hear", "curious about"), and turn ending, with the LLM as final arbiter.

### Gaps
- No found published accuracy numbers for Smart Turn v3/v3.1 (repo says reproducible via `benchmark.py`).
- No found benchmark of question detection specifically on *unpunctuated modern-ASR* text with LLMs (2022–2026).
- No found work on rhetorical-question detection in live speech with numbers.
- The Speechmatics "semantic turn detection" how-to wasn't fetched, so whether it is available for Aside's realtime tier is TBD.

---

## 3. Multi-question handling: extraction, merging, ordering

### Takeaway
Multi-intent detection literature (MixATIS/MixSNIPS) shows LLMs are now competitive with or better than specialised joint models. Absolute "overall accuracy" stays modest on hard sets (~59% on MixATIS for SOTA encoders). For a copilot, a structured-output schema that returns an ordered list of extracted asks plus one consolidated question is the practical design.

### Cited Findings
- MISCA (2023, non-LLM joint intent+slot, RoBERTa-base): overall accuracy 59.1 on MixATIS and 86.2 on MixSNIPS. Prior best baselines were 58.4 / 85.3. — [arXiv 2312.05741](https://arxiv.org/pdf/2312.05741)
- "Do Large Language Models Understand Multi-Intent Spoken Language?" / ECLM (2024): LLMs are "competitive, if not outright superior" to SOTA. ECLM gains +3.7 (MixATIS) and +3.1 (MixSNIPS) overall accuracy over Uni-MIS, and +8.5 / +21.2 over plain supervised LLM fine-tuning. It introduces LM-MixATIS / LM-MixSNIPS. — [arXiv 2403.04481](https://arxiv.org/html/2403.04481v4)
- The reference project in `references/` resolves "what to answer" by answering everything said since the last real question (transcript breakpoint). That is an implicit consolidation strategy. — local file `references/src/shared/ipc.ts`

### Inferences
- **Consolidation rules for Aside**, applied over the "open question window" = Others' speech since the last answered question:
  1. Several asks in one turn ("What's your background, and why this role?") → keep an **ordered list** of sub-questions. Answer with one response that has parts in the same order.
  2. Re-phrasings of the same ask across consecutive turns ("How do you handle conflict? Like, a time you disagreed with a manager?") → **merge** into the most specific version. Don't produce two answers.
  3. A new, unrelated ask that arrives while an answer is streaming → if the user hasn't started speaking, **re-plan**: cancel and regenerate the consolidated answer. If the user is already speaking (Me channel active), **queue** it as the next question.
  4. Context sentences in the window ("We're a 20-person fintech team… so, how would you design our ledger?") → move them into the answer prompt as context, not as questions.
- **Suggested structured output** (JSON schema / tool call, strict mode):
  ```json
  {
    "kind": "question|request|multi_question|follow_up|clarification|context|rhetorical_or_filler|none",
    "complete": true,
    "questions": [{"text": "self-contained rewrite", "type": "behavioral|technical|resume|logistics|other", "refers_to": "q_id|null"}],
    "consolidated": "one question combining all asks, or null",
    "context_notes": ["facts the interviewer shared"],
    "action": "answer|update_answer|note_context|ignore|wait",
    "confidence": 0.0
  }
  ```
  Keep enums short. Put `complete`/`action` first so a streaming parser can act before the rest arrives.

### Gaps
- Absolute LLM accuracy numbers on MixATIS/MixSNIPS weren't in the fetched snippets.
- No found dataset or paper specifically on *consolidating multiple interview/meeting questions* across turns. This would have to be evaluated in-house (see §6).

---

## 4. Follow-ups, coreference and open-question state: conversational query rewriting, topic tracking

### Takeaway
Conversational query rewriting (CQR) is a mature technique: rewrite "and why did you choose that?" into a self-contained question using the dialogue history. LLM rewrites can beat human rewrites for retrieval, and small fine-tuned or distilled rewriters can beat big instructed LLMs. Benchmarks: CANARD, QReCC (14K conversations, 80K QA pairs), TREC CAsT/iKAT.

### Cited Findings
- Ye, Fang, Li & Yilmaz, "Enhancing Conversational Search: LLM-Aided Informative Query Rewriting" (EMNLP Findings 2023). The LLM acts as rewriter with "rewrite-then-edit". Good rewrites need correctness, clarity (resolving coreference and omission), informativeness and non-redundancy. LLM rewrites gave "substantially improved retrieval performance compared to human rewrites" on QReCC, especially with sparse retrievers. They distil into smaller models to cut latency. — [arXiv 2310.09716](https://arxiv.org/pdf/2310.09716); [code](https://github.com/smartyfh/infocqr)
- QReCC: 14K open-domain conversations, 80K QA pairs, each question with a human rewrite. Sources are QuAC, NQ and TREC CAsT-19. — [arXiv 2310.09716 / ar5iv](https://ar5iv.labs.arxiv.org/html/2310.09716)
- Small LLMs fine-tuned for rewriting "can significantly outperform large instructed LLMs such as ChatGPT". The best was a 13B Llama-2 (CNR-ISTI). — [ERCIM News 136](https://ercim-news.ercim.eu/en136/special/enhancing-conversational-search-with-large-language-models)
- A 2021 QReCC shared-task system used T5 fine-tuned on CANARD, with no QReCC tuning. — [arXiv 2201.09146](https://web3.arxiv.org/pdf/2201.09146)
- TREC iKAT (2023–2025, ClueWeb22) provides newer rewriting topics. — [datamaestro IR datasets](https://datamaestro-ir.readthedocs.io/en/stable/datasets/conversation.html)

### Inferences
- **Do rewriting inside the same classification call** (the `questions[].text` field = self-contained rewrite). Then the answer model, and any retrieval over the user's resume/notes, gets a standalone query. That is exactly what CQR is for.
- **Open-question state machine** (per session, in Rust):
  - `Idle` → Others speech starts → `Listening(window)`
  - soft end + classifier says `complete && action=answer` → `Drafting(q)` (speculative)
  - hard EOU → `Committed(q)` → stream answer to overlay
  - new Others speech during Drafting/Committed:
    - classifier says `follow_up` / `clarification` → `update_answer` (regenerate with merged question, keep `refers_to` link)
    - says `context` → append to context, optionally refresh the answer
    - says new `question` → queue or replace per §3
  - Me speech: mark current question "being answered". Feed Me's words back as "what the user already said" so later follow-ups ("can you go deeper on that?") resolve against the user's actual answer, not only the suggestion.
  - Keep a rolling list of the last ~5 Q/A pairs (question rewrite + one-line summary of what Me said) as history for rewriting. That is cheap and bounded.
- **Topic segmentation**: a simple heuristic is enough at first. Start a new "topic" when the classifier marks a new top-level `question` with `refers_to=null`. Older topics then collapse into one-line summaries in the prompt.

### Gaps
- No found 2024–2026 CQR results for *spoken* (ASR-noisy) follow-ups specifically, and no latency numbers for LLM rewriting.
- Topic segmentation for meetings (e.g. AMI/ICSI segmentation with LLMs) wasn't researched in this pass.

---

## 5. Streaming architecture: partial vs final, cascades, one call vs two, speculative generation, latency budgets

### Takeaway
The proven voice-agent pattern is **speculative generation on an early end-of-turn signal, cancel if the speaker resumes, commit on the final end-of-turn**. Deepgram says this costs about 50–70% more LLM calls. Small non-reasoning cloud models land around 0.5–2.5 s TTFT in independent tests. Reasoning modes are far too slow (10–18 s TTFT) for live classification.

### Cited Findings
- Deepgram Flux eager-EOT flow: on EagerEndOfTurn, send the transcript to the LLM and start drafting. On TurnResumed, cancel the draft. On EndOfTurn, finalise. Speculative generation raises LLM calls by about **50–70%**. — [Deepgram eager EOT guide](https://developers.deepgram.com/docs/flux/voice-agent-eager-eot); [Deepgram Flux quickstart](https://developers.deepgram.com/docs/flux/quickstart.mdx); [Telnyx](https://developers.telnyx.com/docs/inference/ai-assistants/transcription-settings)
- LiveKit v1 turn detector removes the "wait for final transcript" step by working on audio. It quotes 295–543 ms mean EOT latency at 10% / 5% false-cutoff targets (vendor benchmark). — [LiveKit blog](https://livekit.com/blog/solving-end-of-turn-detection)
- Independent developer test (2026): Claude Haiku 4.5 ~597 ms TTFT and GPT-4.1 mini ~2,400 ms TTFT on a medium prompt. Single-source and not independently verified. I couldn't fetch the full page (403). — [kunalganglani.com via search summary](https://www.kunalganglani.com/blog/llm-api-latency-benchmarks-2026.md)
- Artificial Analysis lists Claude 4.5 Haiku (reasoning) TTFT at 15.41 s and Gemini 3.5 Flash-Lite at 11.24 s (these include thinking time). — [Artificial Analysis comparison](https://artificialanalysis.ai/models/comparisons/gemini-3-5-flash-lite-vs-claude-4-5-haiku-reasoning)
- The reference project classifies and answers in one call. The answer model emits a hidden length tag first, which the backend strips. — local `references/src/shared/answerLength.ts`

### Inferences
- **Recommended pipeline for Aside** (Rust/Tauri, Speechmatics per channel, cloud LLMs):
  1. **Partials (Others)** → no LLM call. Optionally a cheap local regex/keyword prefilter (wh-word, request verbs, "?") updates a UI "listening to question…" hint.
  2. **Soft end** (~600 ms with no new words on Others, from transcript timing or local VAD/Smart Turn) → **one fast structured call** to a small non-reasoning model (Haiku 4.5 / GPT-4.1-mini or nano / Gemini Flash-Lite with thinking off). Send the last ~5 Q/A history plus the current window, get the §3 JSON back, and cap output at about 150 tokens. If `action=answer && complete`, immediately start the **answer stream** with the main model, but hold it hidden or dimmed (speculative).
  3. **Others resumes speaking** before the hard end → cancel the speculative answer (abort the HTTP stream) and go back to Listening. This is Flux's TurnResumed semantics.
  4. **Hard end** (Speechmatics EndOfUtterance ~1.5 s, or `ForceEndOfUtterance` when the classifier is confident) → if the transcript changed since the soft-end call, re-classify. Otherwise reveal the speculative answer. The user sees an answer starting at about hard-end time, or earlier.
- **Two calls (classify, then answer) vs one call**: two calls give a clean `ignore`/`note_context` decision without paying for an answer, and let classification use the cheapest model. One call (answer model emits a JSON header/tag first, like the reference project's `[[LEN:…]]`) saves one round trip (~0.5–2 s). **Hybrid suggestion**: the cheap classifier runs at soft end and the answer starts speculatively. Effective added latency stays near zero, and the classifier also filters out roughly the "ignore/context" cases before the expensive model streams anything visible.
- **Latency budget (target)**: soft-end detect 0.5–0.7 s → classifier TTFT ~0.4–0.8 s (small model, short prompt) → answer TTFT ~0.6–1.5 s, overlapped with step 2 when speculative. Target first visible words ≤ ~1.5–2.0 s after the interviewer stops. Reasoning/thinking modes are out for this path (10–18 s TTFT per Artificial Analysis).
- **Cost control**: speculative drafting multiplies answer calls by about 1.5–1.7× (Deepgram's figure). Gating speculation on classifier `confidence ≥ ~0.7` and `action=answer` should cut wasted drafts.
- Use **strict JSON schema / tool calling** on whatever provider is used. Keep enum fields first. Use prompt caching for the static system prompt and few-shot examples, which shortens TTFT on repeat calls.

### Gaps
- No verified TTFT numbers for GPT-4.1-nano, GPT-5-mini/nano, Groq-hosted Llama or Gemini Flash-Lite (non-reasoning). The Artificial Analysis pages found were reasoning-mode and the independent blog was unreachable. These should be measured from Aside's own machine and region.
- No found published per-classification cost comparisons. Cost per call depends on current price sheets, which weren't fetched.

---

## 6. Evaluation: building a test set and measuring detection/consolidation accuracy

### Takeaway
No public benchmark matches "live interview copilot question understanding". Build a small in-house set from real (consented) or scripted interview audio run through Speechmatics, label it with the §1 taxonomy, and measure per-class precision/recall, end-of-question timing (false-cutoff rate vs latency, as in eot-bench), and consolidation quality.

### Cited Findings
- eot-bench method: every pause inside a turn is a decision point. Sweep endpointing settings and report the latency vs false-cutoff trade-off, not a single accuracy number. — [LiveKit blog](https://livekit.com/blog/solving-end-of-turn-detection)
- LiveKit reports TPR/TNR for its turn detector: "finished" vs "will continue" correctly identified. — [LiveKit docs](https://docs.livekit.io/agents/logic/turns/turn-detector)
- Rewrite quality criteria (correctness, clarity, informativeness, non-redundancy) can serve as a rubric for consolidated/rewritten questions. — [arXiv 2310.09716](https://arxiv.org/pdf/2310.09716)
- Real ASR noise matters: question detection F1 0.59 on classroom ASR. — [W16-3623](https://preview.aclanthology.org/author-url/W16-3623.pdf)
- Public corpora usable for bootstrapping: ICSI MRDA (meeting DAs incl. questions), SwDA, QReCC/CANARD (follow-up rewriting), MixATIS/MixSNIPS (multi-intent). — [Boakye et al.](https://experts.illinois.edu/en/publications/any-questions-automatic-question-detection-in-meetings/); [QReCC](https://ar5iv.labs.arxiv.org/html/2310.09716); [MISCA](https://arxiv.org/pdf/2312.05741)

### Inferences
- **Test set recipe (about 150–300 items is enough for a 2–3-person project)**:
  - Scripted mock interviews (behavioral, technical, resume walk-through, small talk), recorded through Aside's real capture path so Speechmatics partials/finals/EOU timing are realistic. Include deliberate hard cases: mid-question pauses, imperative requests without "?", rhetorical "right?", stacked questions, re-phrasings, follow-ups that need history, interviewer context monologues, and crosstalk with Me.
  - Save each session's raw Speechmatics event stream (JSONL with timestamps) so it can be **replayed offline** deterministically. Aside's Speechmatics fake server/replay work suggests this fits the existing code.
  - Label per Others-segment: `kind`, `action`, gold self-contained question(s), gold consolidated question, gold "question ended at t".
- **Metrics**:
  - Per-class precision/recall/F1 and a confusion matrix. Most costly errors: `context` → `answer` (noise) and `request` → `ignore` (missed question).
  - End-of-question: false-cutoff rate (answered before the interviewer finished) vs median and P90 latency from true end → first visible answer token, swept over soft-end thresholds (400/600/800 ms).
  - Consolidation: exact sub-question count match, plus an LLM-as-judge or human 1–5 rating on the rewrite rubric (correct, self-contained, complete, non-redundant).
  - Speculation efficiency: % of speculative drafts discarded (aim to stay inside the ~50–70% extra-call envelope Deepgram cites).
- Run the replay suite as an automated regression test whenever the prompt, model or thresholds change.

### Gaps
- No public "interview copilot" benchmark found, and no open-source interview copilot with documented question detection found. The search returned only closed commercial apps (e.g. a Chrome "Interview Copilot" that tags question types with a confidence score). — [Chrome Web Store listing](https://chromewebstore.google.com/detail/ibdllmhlcnecbccejcjbpdldgjnohpoc)
- No published inter-annotator agreement figures for this kind of copilot taxonomy. Expect to measure them in-house.
