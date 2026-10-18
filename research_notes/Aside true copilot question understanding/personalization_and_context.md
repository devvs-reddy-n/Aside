# Personalizing and Grounding a Live Interview/Meeting Copilot: Profile, Memory, Style, Response Policy, Caching, Privacy

Research date: 2026-10-17. Target: Aside (Rust/Tauri desktop, local SQLite with sessions/transcripts/documents/facts/chunks + FTS5, cloud LLM mainly OpenAI, per-session "copilots" holding resume, job description, company, rules).

Note on source quality: the strongest sources here are vendor docs (OpenAI and Anthropic prompt caching, Anthropic context engineering, contextual retrieval, hallucination guidance) and arXiv papers (MemGPT, Mem0, Zep, LaMP, Lost in the Middle). Memory-vendor numbers (Mem0, Zep) come from the vendors' own papers and are disputed by competitors. Interview-coaching (STAR) sources are career blogs, which are weaker. Answer-shape rules for each kind of utterance had almost no published research behind them; that section is mostly design inference, labeled as such.

---

## 1. User profile and grounding (resume, JD, company, stories; RAG vs. putting everything in the prompt; hallucinated experience)

### Takeaway
A single copilot's material is small: a resume, a job description, company notes and roughly 10–20 stories add up to a few thousand tokens, far under the roughly 200k-token level where Anthropic says you can skip retrieval altogether. So put the whole structured profile, in a fixed form, in the cached top part of the prompt, and save retrieval (FTS5/BM25 plus optional embeddings) for large extras: long documents, past session transcripts and big libraries. Grounding rules decide whether answers are trustworthy: (a) explicitly allow "not in your profile," (b) only claim experience that appears in the profile, and (c) tag claims with profile item IDs so the app can check them.

### Cited Findings
- Anthropic: if a knowledge base is "smaller than 200,000 tokens" (about 500 pages), you can skip RAG and include the whole knowledge base in the prompt. Prompt caching makes this "over 2x" faster and up to 90% cheaper. — [Anthropic, Introducing Contextual Retrieval (Sep 19, 2024)](https://www.anthropic.com/news/contextual-retrieval)
- When retrieval is needed, Anthropic measured retrieval failure (top-20, baseline 5.7%):
  - contextual embeddings: −35% (to 3.7%)
  - contextual embeddings + contextual BM25: −49% (to 2.9%)
  - adding reranking: −67% (to 1.9%)
  - Retrieving the top 20 chunks beat top 5 and top 10. With reranking, they first retrieve 150 and pass the top 20.
  - Chunks are typically "no more than a few hundred tokens" (the cost example uses 800-token chunks), and the added context per chunk is 50–100 tokens.
  - The one-time cost to contextualize is about $1.02 per million document tokens when using prompt caching.
  — [Anthropic, Contextual Retrieval](https://www.anthropic.com/news/contextual-retrieval)
- Anthropic context engineering (Sep 29, 2025):
  - Recommends a hybrid approach: "retrieving some data up front for speed," plus just-in-time loading through lightweight identifiers.
  - Principle: "find the smallest set of high-signal tokens that maximize the likelihood of your desired outcome."
  - Context rot: recall accuracy drops as the context grows, and "every new token introduced depletes this budget."
  — [Anthropic, Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- Position matters. Accuracy is highest when the relevant information sits at the start or end of the input and drops in the middle. It also drops as the input grows, even for long-context models. In the 20–30-document setting, GPT-3.5-Turbo fell below its closed-book accuracy. — [Liu et al., Lost in the Middle, TACL vol. 12 (2024), arXiv July 2023](https://arxiv.org/abs/2307.03172v3); [ACL Anthology](https://preview.aclanthology.org/setup/2024.tacl-1.9)
- LaMP benchmark (Salemi et al., v1 Apr 2023, v4 Jun 2024): 7 personalization tasks (3 classification, 4 generation). Retrieving items from the user's own profile to personalize outputs was effective in both zero-shot and fine-tuned settings. The authors compared term matching (BM25-style), semantic matching and time-aware retrievers. — [arXiv 2304.11406](https://arxiv.org/abs/2304.11406). (The abstract page gives no per-retriever numbers; see Gaps.)
- Anthropic's hallucination-reduction guidance:
  - Explicitly allow "I don't know"; this "can drastically reduce false information."
  - For documents over 20k tokens, extract word-for-word quotes first.
  - Cite a supporting quote for every claim, and retract any claim without one.
  - Explicitly restrict answers to the provided documents ("external knowledge restriction").
  - Best-of-N comparison catches inconsistent answers.
  — [Anthropic docs, Reduce hallucinations](https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/reduce-hallucinations)
- LangChain memory concepts frame a user profile as one of two shapes:
  - A "profile" is "generally just a JSON document with various key-value pairs." It is easy to inject, but becomes error-prone to update as it grows.
  - A "collection" holds many narrowly scoped records. Each is easier to generate and less likely to lose information, but harder to update and search.
  — [LangChain docs, Memory concepts](https://docs.langchain.com/oss/python/concepts/memory)

### Inferences
- **Recommended profile shape for Aside, a hybrid of "profile" and "collection":**
  - Store a structured `profile` per copilot, parsed once from the resume, JD and company text by an LLM extraction pass with a JSON schema, then reviewed and edited by the user.
  - Record types (each with a stable ID such as `R3`, `S7`, `J2`):
    - **Roles:** company, title, dates, scope.
    - **Projects/achievements:** what was done, the user's specific action, metrics, tech used.
    - **Skills:** each with a level and evidence IDs.
    - **Stories (STAR):** title, Situation, Task, Action, Result with a metric, themes/competencies such as conflict, failure, leadership or ambiguity, plus which roles or projects the story maps to.
    - **Talking points / "why this company" / questions to ask.**
    - **JD requirements:** each requirement, with links to the evidence that covers it, and any gaps.
    - **Company facts:** product, stack, values, recent news.
  - Each record is small: roughly 30–120 tokens. Estimated whole-profile size is 2–6k tokens (resume about 600–1,200, JD about 500–1,000, company 300–800, 10–20 stories at 100–200 each). These are estimates, not measured.
- **Put the whole profile in the prompt rather than retrieving it.** It is far below the roughly 200k "skip RAG" level and well above the minimum size for caching (1,024 tokens on OpenAI GPT-5.6+, 512–4,096 on Claude depending on model). So it should sit in the cached prefix: cheaper and faster than doing a retrieval step on every utterance, and it never misses the one story that matters.
  - Use FTS5 retrieval only for long uploaded documents (papers, design docs, prior transcripts) and for cross-session memory. Optionally add embeddings later for hybrid search, with BM25 (FTS5 `bm25()`) as the first stage, since Anthropic's results show BM25 adds a large gain.
- **Chunking a resume:** split by meaning, not by token count. Use one chunk per role, per project bullet group and per story. Prefix each chunk with its context, e.g. "Role: Senior SWE at Acme 2021–2024 — …"; this is the cheap equivalent of Anthropic's contextual chunk prefix, and resume bullets are meaningless without the role around them. In practice, this is what the structured profile records already are.
- **Grounding rules for the system prompt:**
  - "Speak as the user in first person, using ONLY experience, employers, numbers, and tools that appear in <profile>. Never invent metrics, employers, titles, or dates."
  - "If the question asks for experience the profile lacks, say so briefly, then offer (a) the closest real adjacent experience by ID, or (b) an honest 'haven't done X directly, here's how I'd approach it' answer."
  - "Tag each factual claim with its source ID in a hidden field (`sources: ["S7","R2"]`)."
  - The Rust side can then check that every cited ID exists, and can flag answers containing numbers that appear nowhere in the profile (a cheap regex check). This is Anthropic's "verify with citations" idea, adapted for speed: check IDs and numbers rather than running a second LLM pass.
- **Order within the prompt** (from Lost in the Middle): stable rules, then the profile, then the conversation state, then the recent transcript, with the current question last. The current question should sit at the very end, closest to where the answer is generated.

### Gaps
- No published benchmark was found that measures hallucinated-experience rates specifically for interview copilots.
- LaMP's per-retriever improvement numbers were not checked; only the abstract was read.
- No measured token counts for typical resumes or JDs were found; the sizes above are estimates.

---

## 2. Live conversation memory (rolling summary, structured conversation state, fact extraction, memory architectures, 1-hour call budgets)

### Takeaway
For a single 1-hour call, the raw transcript is small enough to keep in large part. The better design is layered:
1. Keep the last few minutes word-for-word.
2. Keep a structured "conversation state" that a cheap background model updates every N turns (facts the interviewer shared, topics covered, what the user already claimed, open questions).
3. Optionally keep a rolling summary of older turns.

Write memory in the background, not in the path that answers questions, so answers stay fast. Heavy memory frameworks (MemGPT/Letta, Mem0, Zep) target memory across many sessions. Their ideas are worth borrowing (extract → merge → retrieve; time-stamped facts that are invalidated rather than deleted), but none of them needs to be adopted as a dependency.

### Cited Findings
- **MemGPT** (Packer et al., Oct 2023, rev. Feb 2024) treats the context window like an operating system's memory hierarchy. "Virtual context management" moves data between fast storage (inside the context) and slow storage (outside it), and the model uses interrupts to control flow. It was evaluated on documents larger than the context window and on multi-session chat. — [arXiv 2310.08560](https://arxiv.org/abs/2310.08560)
- **Mem0** (Chhikara et al., Apr 28, 2025):
  - "Dynamically extracting, consolidating, and retrieving salient information" from conversations.
  - Reports +26% relative on an LLM-as-judge metric vs. OpenAI's memory, 91% lower p95 latency than full context, and >90% token savings. The graph variant adds about 2%.
  — [arXiv 2504.19413](https://arxiv.org/abs/2504.19413)
  - Mem0's own guide: on LOCOMO, full-context uses >25k tokens per call vs. <7k for Mem0, and p95 latency drops from 17.12 s to about 1.44 s. — [Mem0 guide](https://guide.mem0.ai/faq/benchmarking-cost-difference-full-llm-context-compressed-memory)
  - **Caveat:** Letta and Zep have publicly challenged Mem0's benchmark method, and every memory add or update costs an extra LLM call. — [rywalker.com review](https://rywalker.com/research/mem0)
- **Zep/Graphiti** (Rasmussen et al., Jan 2025): a temporal knowledge graph that updates facts without discarding old ones and tracks when each fact was valid. Reports 94.8% vs. MemGPT's 93.4% on Deep Memory Retrieval, and on LongMemEval up to 18.5% higher accuracy with about 90% lower latency vs. baselines. — [arXiv 2501.13956](https://arxiv.org/html/2501.13956v1); [Zep blog](https://blog.getzep.com/zep-a-temporal-knowledge-graph-architecture-for-agent-memory)
  - It uses a "bi-temporal" model: it records both when something happened and when Zep learned it, and marks outdated facts invalid instead of deleting them. — secondary: [aktagon summary](https://signals.aktagon.com/articles/2026/07/zep-a-temporal-knowledge-graph-architecture-for-agent-memory/)
- **LangChain/LangGraph memory concepts:**
  - Short-term memory is scoped to one conversation thread; long-term memory lives in namespaces shared across sessions.
  - Memory types: semantic (facts), episodic (past experiences, often used as few-shot examples) and procedural (rules and instructions).
  - Writing memory "in the hot path" gives real-time availability, but "the process of reasoning about what to save to memory can impact agent latency."
  - Writing in the background "eliminates latency in the primary application," at the cost of slower updates.
  — [LangChain docs, Memory](https://docs.langchain.com/oss/python/concepts/memory)
- **Anthropic context engineering:**
  - Compaction means summarizing a near-full context and starting again from the summary. Tune it to maximize recall first, then precision.
  - "Structured note-taking": the agent "regularly writes notes persisted to memory outside of the context window."
  — [Anthropic, Sep 29, 2025](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- **OpenAI caching caveat:** "compaction, summarization, or truncation can reset reuse." Appending new messages keeps the cache prefix; rewriting earlier turns breaks it. — [OpenAI Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)

### Inferences
- **Budget for a 1-hour call** (estimate, not measured): conversational speech is about 130–160 words per minute, so 60 minutes is roughly 8–10k words, or about 11–14k tokens of transcript. Keeping the full transcript is feasible, but costly on every request unless cached, and it suffers from context rot and lost-in-the-middle. A good budget per answer request:
  - rules: about 1–2k tokens
  - profile: 2–6k
  - conversation state: 0.5–1.5k
  - recent transcript, word-for-word (last 3–6 minutes, or the last 8–15 turns): 1–3k
  - question: under 200
  - **Total: roughly 5–12k input tokens**, where the first 3–8k is a stable, cacheable prefix.
- **Conversation state schema** (a JSON row in SQLite per session, updated by a background job after each completed interviewer turn, or every 30–60 s):
  - `interviewer_facts[]` — {fact, category: team / stack / role_challenge / process / culture / logistics, turn_id, ts, superseded_by?}, e.g. "team is 6 engineers," "uses Kafka + Go," "biggest challenge: migrating off monolith."
  - `user_claims[]` — {claim, profile_ids[], turn_id}. Covers what the user has already said: stories used, numbers stated, opinions given. This is the consistency memory.
  - `topics_covered[]` — topic plus the IDs of stories used, so the same story isn't reused for different questions unless that's intended.
  - `open_threads[]` — questions the interviewer said they'd come back to, parts of a multi-part question not yet answered, and good questions for the user to ask at the end.
  - `interviewer_profile` — names and roles mentioned, plus their apparent priorities.
  - `phase` — intro / behavioral / technical / system design / candidate questions / wrap-up.
- **Updating the state:** use a small, cheap model with structured outputs (JSON schema) on the newly finished turns plus the current state. Have it emit add/update/supersede operations, the same pattern Mem0 and Zep use: merge rather than append, and mark contradicted facts as replaced instead of deleting them. Run this off the answer path. Persist the state to SQLite so it survives a crash, and save the final version for cross-session memory.
- **Rolling summary:** needed only once the call goes past roughly 20–30 minutes. Summarize the oldest word-for-word block into about 300–600 tokens and put it after the cached prefix, so rebuilding the summary does not break the cache (see Section 5).
- **Memory across sessions:** after a session ends, promote its `user_claims` (which stories were told to which company, which numbers were stated) and the final interviewer facts into the copilot's long-term store. That way the second-round interview stays consistent with the first. FTS5 over past sessions is enough to start; the time-validity marking from Zep is the main idea worth copying.
- **Don't adopt Letta/Mem0/Zep as dependencies.** They are Python/TypeScript services, built for many-session agents, and their write paths add an extra LLM call each. Aside needs one session's state plus a simple store across sessions, which fits in a few SQLite tables.

### Gaps
- No public engineering write-ups were found from commercial meeting assistants (Otter, Fireflies, Granola, Cluely, Final Round AI) describing their actual context-window strategy or token budgets.
- Mem0's internal ADD/UPDATE/DELETE/NOOP operation set is widely described, but it was not confirmed from the pages read here.
- No source measured how update frequency for the conversation state trades off against answer quality.

---

## 3. Style personalization (voice, tone, length, learning from feedback; spoken first-person format; STAR; code/system-design formats)

### Takeaway
Use the user's own words (their stories, written in their voice) as the main source of style, plus 2–3 short examples of answers they approved. Few-shot examples teach style better than long lists of rules. Use a fixed layout that is easy to read aloud: a one-line direct answer, then 2–4 short bullets in first person. Behavioral questions use compact STAR with a number in the Result, sized to be spoken in about 60–120 seconds.

### Cited Findings
- Anthropic: prefer "a set of diverse, canonical examples that effectively portray the expected behavior" over long lists of edge-case rules. Start with a minimal prompt and add instructions only in response to observed failures; aim for the "right altitude" between hardcoded if-else rules and vague guidance. — [Anthropic, Effective context engineering (Sep 29, 2025)](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- LangChain: episodic memory (past experiences) is "often used through few-shot examples," and procedural memory is the rules or system prompt, which can be updated from feedback. — [LangChain docs, Memory](https://docs.langchain.com/oss/python/concepts/memory)
- LaMP: retrieving the user's own past items into the prompt personalizes generated text across several generation tasks. — [arXiv 2304.11406](https://arxiv.org/abs/2304.11406)
- STAR length and structure (career guides, weaker sources):
  - Target about 90 seconds to 2 minutes; answers under a minute lack depth.
  - Keep Situation and Task to about 2–3 sentences combined; Action should be the longest part.
  - Say "I," not "we," in the Action.
  - Put a number in the Result (%, $, time saved), or a lesson learned if there is no number.
  — [Glassdoor STAR guide (mirror)](https://image4.glassdoor.co.in/blog/guide/star-method/); [Creately](https://creately.com/guides/what-is-the-star-method-for-interviews/); [orbytjobs](https://www.orbytjobs.ai/blog/star-method-interview)
  - Glassdoor also warns that "a rigid, number-driven delivery" can hurt otherwise solid content. — [Glassdoor mirror](https://image4.glassdoor.co.in/blog/guide/star-method/)
- Local reference code: the project's `references/` folder (a local-first Electron meeting assistant, "Assistant") has a per-mode "casual" tone directive and separate "humanize"/answer-length modules. Its rules include: "Lead with the takeaway, then the why. No preamble"; first person; "no big bullet dumps"; and "Casual is not made-up. Stay accurate and grounded." It teaches the style with one stiff-vs-casual example pair. — local file `references/src/shared/casual.ts` (also `answerLength.ts`, `humanize.ts`, `ending.ts`, `modes.ts`)

### Inferences
- **Style profile** (stored per user, injected inside the cached prefix): about 150–300 tokens. Contents:
  - target spoken length in seconds or words
  - formality
  - phrases the user likes and phrases to avoid (e.g. "leverage," "synergy")
  - preferred opening style
  - accent/ESL readability preference (short sentences, common words)
  - 2–3 approved example answers, written in the user's voice
- **Learning from feedback:**
  - Add thumbs up/down and a "too long / too formal / not me" quick tag on each answer, plus "edit and save as my version."
  - Store these per user. Periodically update the style profile from them: a small offline LLM job turns the tags into style rules, and the best approved answers are kept as examples. This updates the procedural and episodic memory.
  - Keep examples to 2–3 (about 300–600 tokens) to protect the cache and the attention budget.
- **Default answer layout:**
  - Line 1: a direct one-sentence answer the user can say immediately.
  - Lines 2–5: 2–4 bullets, each a spoken-length phrase (about 8–20 words), each holding one concrete fact from the profile.
  - Optional final line: a bridge back to the role, e.g. "That's similar to the migration you mentioned."
  - No markdown headers, no hedging preamble. Contractions are fine.
- **Behavioral (STAR):** a 1-line hook naming the story, then S and T (1 line), A (2–3 bullets beginning with "I…"), and R (1 line with a metric from the profile). Pick the story from the profile by its competency tags, and avoid stories already in `topics_covered`.
- **Technical / coding:**
  - Give the approach in 1 line, then key steps, then complexity, then a code block only if the question asks for code.
  - Mention a related profile project if one exists.
- **System design:**
  - Clarifying questions to ask first (2–3), a high-level component list, data flow, scale numbers, trade-offs.
  - Tie it to the interviewer's stated stack from `interviewer_facts`.

### Gaps
- No peer-reviewed study was found on the best on-screen answer length or format for being read aloud in real time.
- No evaluation was found of style-profile versus few-shot approaches specifically for spoken answers.
- STAR timing guidance comes from career blogs, not research.

---

## 4. Response policy for each type of utterance (information shared, small talk, follow-up, multi-part questions, clarification)

### Takeaway
The model should first label what the other person just said, then pick a matching answer shape. Many utterances are not questions. When the interviewer shares a fact, the best output is a tiny "Noted" card that links the fact to the user's profile; this also feeds the conversation state. Doing the labeling and the answer in one structured-output call keeps it fast.

### Cited Findings
- Anthropic recommends minimal, unambiguous instructions and canonical examples over brittle if-else rules. That favors a short policy table plus one example per type, rather than long branching rules. — [Anthropic, Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- OpenAI Structured Outputs (`text.format` with a JSON schema) puts the schema into the cached prefix, so a fixed schema costs nothing extra per request once cached. Changing the schema breaks the cache. — [OpenAI Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)
- The local reference repo keeps separate tone, length and ending modules (`casual.ts`, `answerLength.ts`, `ending.ts`) and switches them per "mode." That points to a mode/policy layer kept separate from the base prompt. — local `references/src/shared/`

### Inferences (design; no published research found)
- **Output schema (one call):**
  - `{ type, confidence, display: {headline, bullets[], code?}, notes_for_state?: {...}, sources[] }`
  - `type` is one of: question_behavioral | question_technical | question_system_design | question_general | multi_question | follow_up | clarification_request | info_shared | small_talk | logistics | not_addressed_to_user.
  - When speed matters, stream `display.headline` first.
- **Answer shape for each type:**
  - **info_shared** ("we're a team of six and we run everything on Kafka"):
    - No answer.
    - Show a 1-line card: "Noted: team of 6, uses Kafka → tie in your Kafka ingestion project (P4)."
    - Add the fact to `interviewer_facts`.
    - At most one optional "you could say" line, e.g. "Oh nice, I used Kafka at Acme for…".
  - **small_talk** ("how's your day going?"): one natural line under 15 words, warm and in first person. Don't pull from the profile unless it fits naturally (e.g. "glad to finally chat — I've been reading about your Series B").
  - **follow_up** ("what was the hardest part of that?"):
    - Resolve "that" using the last `user_claims` entry and the recent transcript.
    - Stay consistent with the numbers and story already told: reuse the same story ID and add depth, without repeating what was already said.
    - Shorter than a fresh answer: a headline plus 1–2 bullets.
  - **multi_question** ("tell me about X, and also how you'd handle Y"):
    - Number each part in the order asked: "1) … 2) …", each with a headline plus 1–2 bullets.
    - If a part can't be answered, put it in `open_threads`.
  - **clarification_request** (the interviewer asks the user to clarify, or the question itself is ambiguous): suggest a quick clarifying line the user can say ("Just to check — do you mean at the API layer or the data layer?") plus a fallback answer for the most likely reading.
  - **logistics** (next steps, salary, availability): a short, careful answer. Use the user's rules from the copilot (e.g. a salary range or deferral script). Never invent numbers.
  - **not_addressed_to_user** (the interviewer talking to a colleague, or thinking aloud): show nothing, or only a quiet note.
- **Confidence gating:** if `confidence` is low on whether something is a question at all, show the "Noted" form rather than a full answer, to avoid distracting the user.
- **Use earlier context:** each answer should include a bridge only when a fact the interviewer stated actually matches a profile item. A precomputed match list (interviewer facts × profile skills), updated in the background, makes this cheap.

### Gaps
- No academic or industry source was found that defines dialogue-act classes or answer policies specifically for live interview copilots. The classes above are adapted from general dialogue-act ideas and product reasoning.
- No measured comparison of one-call versus two-call (classify then answer) latency for this task was found.

---

## 5. Context engineering and prompt caching (static vs. dynamic ordering, token budgets, latency, structured outputs)

### Takeaway
Order the prompt from most stable to least stable: tools/schema, then system rules, then the style profile, then the copilot profile (resume/JD/company/stories), then the session's slowly changing state, then the recent transcript, then the current question. Keep the first several thousand tokens byte-for-byte identical within a session. On OpenAI, cached input costs 0.1× (GPT-5.6+) and cuts time-to-first-token. Keep that prefix stable, add new turns at the end, and put the rolling summary after the cache breakpoint.

### Cited Findings
- **OpenAI prompt caching** (undated docs page, current as of fetch; it references GPT-5.5/5.6/6.1 models):
  - On by default. Caches the shared prefix, and a hit requires "the entire rendered prefix to match."
  - Minimum is 1,024 visible input tokens on GPT-5.6+. On earlier models it varies, and cached tokens are rounded down to the nearest multiple of 128.
  - GPT-5.6+ supports up to 4 cache writes per request through explicit `prompt_cache_breakpoint` or implicit breakpoints (at the end of the latest eligible message). GPT-5.5 uses implicit 2,048-token intervals.
  - Cost: cached reads are discounted "up to 95%" — 0.1× input on GPT-5.6+ (0.05× on GPT-6.1 Sol). Writes cost 1.25× on GPT-5.6+; earlier models have no separate write charge.
  - Retention: at least 30 minutes after the last use on GPT-5.6+. Earlier models: `in_memory` lasts about 5–10 minutes idle (up to 1 hour); `24h` lasts up to 24 hours. Organizations with Zero Data Retention (ZDR) default to `in_memory`.
  - `prewarm: true` writes the cache without generating output.
  - Traffic above about 15 requests per minute per prefix can overflow to other machines. Before GPT-5.6, use a stable `prompt_cache_key` and keep about 15 requests per minute per key.
  - Ordering: stable content first, dynamic content last. Append messages instead of rewriting them. Keep tool definitions and schemas stable, and use `tool_choice`/`allowed_tools` instead of removing tools. Compaction or summarization resets reuse.
  - Monitoring: watch `usage.input_tokens_details.cached_tokens` and `cache_write_tokens`; cached tokens still count toward TPM limits. Example deployments reach about 70% and over 90% hit rates.
  — [OpenAI Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)
- **Anthropic prompt caching:**
  - Prefix order is `tools` → `system` → `messages`, with up to 4 `cache_control` breakpoints.
  - Minimum cacheable length: 512 tokens on newer models (Opus 5.5, Sonnet 5.5, Haiku 5.5…), up to 4,096 on some older ones.
  - TTL: 5 minutes by default (refreshed free on each hit), or 1 hour. Writes cost 1.25× (5 min) or 2× (1 hour); reads cost 0.1× (0.05× on Opus/Sonnet 5.5).
  - Cache hits cut time-to-first-token but do not change output speed. A cache entry is usable only after the first response begins, so parallel first requests miss.
  - Changing tool definitions invalidates everything. Changing `tool_choice` or images invalidates the system and messages layers.
  — [Anthropic docs, Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- With prompt caching, putting the full knowledge base in the prompt cut latency by >2× and cost by up to 90% vs. no caching. — [Anthropic, Contextual Retrieval](https://www.anthropic.com/news/contextual-retrieval)
- Context rot and lost-in-the-middle argue for keeping the volatile, most relevant material (the latest question) at the end. — [Anthropic context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents); [Liu et al.](https://arxiv.org/abs/2307.03172v3)

### Inferences
- **Prompt layout for Aside (OpenAI Responses API):**
  1. Fixed tools / `text.format` JSON schema — identical for every answer request.
  2. Developer message A, global rules (grounding rules, response policy table, examples for each answer type): about 1–2k tokens, the same for all sessions.
  3. Developer message B, user style profile plus 2–3 approved examples: changes rarely.
  4. Developer message C, copilot profile (structured resume/JD/company/stories, serialized in a stable order with IDs): fixed for the session. **Cache breakpoint here.** The prefix is now about 4–9k tokens and is reused on every request in the session.
  5. Session state (conversation state JSON plus rolling summary): changes every minute or so. Put a second breakpoint after it if it is large, so it is reused between updates.
  6. Recent transcript, word-for-word, then the current utterance, marked clearly as `<current>`.
- **Stable serialization matters.** Serialize the profile deterministically: sorted keys, no timestamps, no "current time" in the prefix. Even a one-byte difference loses the whole cache.
- **Prewarm at session start.** When the user presses Start, send a `prewarm` request (GPT-5.6+) or a tiny `max_tokens` request containing layers 1–4. The first real question then hits a warm cache. On GPT-5.6+ the 30-minute TTL refreshes with each use, so a 1-hour call stays warm as long as requests come at least every 30 minutes.
- **Token and cost estimate** (illustrative, not measured): with an 8k prefix and 3k of dynamic content per request, at about 60 requests per hour, cached reads at 0.1× bring the effective input cost to about 0.1×8k + 3k = 3.8k token-equivalents per request, instead of 11k. That is about a 65% input-cost reduction, plus a lower time-to-first-token on the 8k prefix.
- **Background model calls** (state updater, "Noted" extraction) should use their own fixed prefix and a small/mini model, so they don't break the answer path's cache. They need no `prompt_cache_key` sharing with the answer path.
- **Structured outputs:** keep one fixed schema for the answer call so the schema stays cached. Stream it, and render `headline` first. Avoid changing `allowed_tools` or the schema during a session.

### Gaps
- OpenAI's current page gives no specific latency-reduction percentage for cache hits (earlier versions of the page did; not re-checked here).
- No independent measurements were found of time-to-first-token with and without caching at 5–10k-token prefixes for current GPT-5.x models. This is worth measuring directly in Aside by logging `cached_tokens` and TTFT.

---

## 6. Privacy of storing personal profiles and transcripts locally

### Takeaway
Local SQLite storage is the strongest privacy position available, but every answer still sends the profile and transcript to the cloud LLM. Provider-side caching also keeps prompt computations for minutes to hours, and up to 24 hours by default on some OpenAI models for organizations without ZDR. Tell users plainly what leaves the machine, offer retention controls and per-copilot deletion, and consider encrypting data at rest.

### Cited Findings
- OpenAI caches store KV tensors (the model's saved computation for the prompt), not raw tokens. Caches aren't shared across organizations. For models supporting both modes, organizations without ZDR default to `24h` retention and ZDR organizations default to `in_memory`. Manual cache clearing is not available. — [OpenAI Prompt caching guide](https://developers.openai.com/api/docs/guides/prompt-caching)
- Anthropic cache entries last 5 minutes by default or 1 hour optionally. — [Anthropic docs, Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- The local reference app markets itself as "Everything runs on this machine… SQLite for persistence. No cloud, no API key." This is the fully local extreme, using Ollama for inference. — local `references/README.md`

### Inferences
- **What to tell the user** (plain language): "Your resume, stories and the live transcript are saved only on this Mac. To write each suggestion, the relevant parts are sent to OpenAI. OpenAI may briefly keep a processed form to speed up the next request."
- **Controls:**
  - Per-session "don't save transcript" option.
  - Auto-delete transcripts after N days.
  - Delete a copilot, which removes its profile, facts and chunks, plus a full `VACUUM` so deleted data is actually gone from the file.
  - Export.
  - Remove third-party personal details (the interviewer's name and contact info) from long-term memory unless the user keeps them.
- **At rest:** encrypt the SQLite database (e.g. SQLCipher) with a key in the macOS Keychain, or at least encrypt sensitive tables. Keep API keys in the Keychain, never in SQLite.
- **Consent:** recording the other party raises consent questions that vary by jurisdiction. Out of scope here, but worth a separate check.
- **Minimize what is sent:** send only the structured profile and the conversation state, not raw uploaded documents (which may hold addresses, phone numbers and references). Strip contact details from the resume before it goes into the prompt; the model never needs your phone number.

### Gaps
- OpenAI's and Anthropic's general API data-retention and training policies (as opposed to cache retention) were not fetched in this pass; the report writer should not state them from these notes.
- Recording-consent law was not researched.
- **Repo comparison gap:** CLAUDE.md asks that features be compared against `references/Repo1-Natively` and `references/Repo2-OpenCluely`. Neither directory exists in the working tree. `references/` contains a single different project ("Assistant," local-first Electron + Ollama), from which a few observations are cited above. How Natively and OpenCluely handle profile, memory and answer policy could not be checked.
