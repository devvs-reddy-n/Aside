import { useEffect, useMemo, useState } from "react";
import * as api from "../../bridge/api";
import type { PastSessionSummary, SessionSummary, TranscriptSearchResult } from "../../bridge/types";
import { conversationText, mergeConversation, type ConversationLine } from "../../core/conversation";
import { downloadPdf, downloadText } from "../../core/downloads";
import { formatClock, formatDate, formatDuration, formatRelative, plural, slug } from "../../core/format";
import { useStore } from "../../core/store";
import { Badge, Callout, EmptyState, Spinner } from "../../kit/Bits";
import { Button } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Segmented } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { toast } from "../../kit/toast";
import { deleteWithUndo } from "../../kit/undo";
import { historyStore, loadHistory, returnToCall, setHidden, systemStore } from "../data";
import { hrefFor, navigate } from "../router";

export function HistoryPage({ selectedId }: { selectedId: string | null }) {
  const { sessions, loaded, error, hiddenIds } = useStore(historyStore, (state) => state);
  const activeId = useStore(systemStore, (state) => state.active?.id ?? null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<TranscriptSearchResult[]>([]);
  const [searching, setSearching] = useState(false);

  useEffect(() => {
    void loadHistory();
  }, []);

  // Search inside what was actually said, not just titles. Waits for a pause in typing.
  useEffect(() => {
    const term = query.trim();
    if (term.length < 2) {
      setHits([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    let alive = true;
    const timer = window.setTimeout(() => {
      void api.history.search(term).then((result) => {
        if (!alive) return;
        setHits(result.ok ? result.value : []);
        setSearching(false);
      });
    }, 300);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [query]);

  const visible = useMemo(() => sessions.filter((session) => !hiddenIds.includes(session.id)), [sessions, hiddenIds]);
  const matching = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return visible;
    return visible.filter((session) => `${session.title} ${session.company ?? ""}`.toLowerCase().includes(term));
  }, [visible, query]);
  const selected = selectedId ? visible.find((session) => session.id === selectedId) ?? null : null;

  function remove(session: PastSessionSummary) {
    if (selectedId === session.id) navigate("history");
    deleteWithUndo({
      label: `“${session.title}”`,
      hide: () => setHidden(historyStore, session.id, true),
      restore: () => setHidden(historyStore, session.id, false),
      commit: async () => {
        const result = await api.history.remove(session.id);
        if (result.ok && result.value) {
          await loadHistory();
          setHidden(historyStore, session.id, false);
        }
        return result.ok && result.value;
      },
      onFailed: (message) => toast(message, { tone: "bad" }),
    });
  }

  return (
    <div className="hm-page">
      <header className="hm-page__head">
        <div>
          <h1>History</h1>
          
        </div>
      </header>

      {error ? <Callout tone="bad" title="Couldn't load your history">{error}</Callout> : null}

      {visible.length > 0 ? (
        <label className="hm-search hm-search--wide">
          <Icon name="search" size={16} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search calls" aria-label="Search history" />
          {searching ? <Spinner size={14} /> : <span className="hm-search__count">{plural(visible.length, "call")}</span>}
        </label>
      ) : null}

      {loaded && visible.length === 0 ? (
        <EmptyState icon="clock" title="No calls yet" action={<Button variant="primary" onClick={() => navigate("copilots")}>Go to Copilots</Button>}>
          Finished sessions appear here.
        </EmptyState>
      ) : (
        <>
          {hits.length > 0 ? (
            <section aria-label="Found in conversations" className="hm-hits">
              <p className="caption">Found in conversations</p>
              <ul className="hm-files">
                {hits.slice(0, 20).map((hit, index) => (
                  <li key={`${hit.session_id}-${hit.start_ms}-${index}`}>
                    <a className="hm-hit" href={hrefFor("history", hit.session_id)}>
                      <span className="hm-hit__who">{hit.speaker === "you" ? "You" : "Others"}</span>
                      <Highlight text={hit.text} term={query.trim()} />
                      <small>{hit.session_title}</small>
                    </a>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          <ul className="hm-files" aria-label="Calls">
            {matching.map((session) => {
              const live = session.id === activeId;
              return (
                <li key={session.id} className="hm-file">
                  <a className="hm-file__main" href={hrefFor("history", session.id)}>
                    <span className="hm-file__icon"><Icon name="clock" size={18} /></span>
                    <span className="hm-file__text">
                      <b>{session.title}</b>
                      <small>{[session.company, formatDate(session.started_at_ms), formatDuration(session.duration_ms)].filter(Boolean).join(" · ")}</small>
                    </span>
                    {live ? <Badge tone="ok">Live now</Badge> : session.status === "draft" ? <Badge>Draft</Badge> : session.segment_count === 0 ? <Badge>No transcript</Badge> : null}
                  </a>
                  {live ? <Button size="sm" variant="secondary" onClick={() => void returnToCall()}>Return to call</Button> : null}
                  <button type="button" className="icon-btn icon-btn--sm icon-btn--danger" aria-label={`Delete ${session.title}`} title="Delete" onClick={() => remove(session)} disabled={live}>
                    <Icon name="trash" size={16} />
                  </button>
                </li>
              );
            })}
            {loaded && matching.length === 0 && visible.length > 0 && hits.length === 0 && !searching ? <li className="hm-muted hm-pad">No call matches “{query}”.</li> : null}
          </ul>
        </>
      )}

      {selected ? <SessionSheet session={selected} onClose={() => navigate("history")} onDelete={() => remove(selected)} /> : null}
    </div>
  );
}

function Highlight({ text, term }: { text: string; term: string }) {
  const index = term ? text.toLowerCase().indexOf(term.toLowerCase()) : -1;
  if (index < 0) return <span className="hm-hit__text">{text}</span>;
  return (
    <span className="hm-hit__text">
      {text.slice(0, index)}<mark>{text.slice(index, index + term.length)}</mark>{text.slice(index + term.length)}
    </span>
  );
}

/* ------------------------------ one call's sheet ----------------------------- */

type Tab = "conversation" | "notes" | "summary";

function SessionSheet({ session, onClose, onDelete }: { session: PastSessionSummary; onClose: () => void; onDelete: () => void }) {
  const [tab, setTab] = useState<Tab>("conversation");
  const [lines, setLines] = useState<ConversationLine[] | null>(null);
  const [notes, setNotes] = useState<string[] | null>(null);
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [summarizing, setSummarizing] = useState(false);
  const [summaryError, setSummaryError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void Promise.all([api.history.transcript(session.id), api.history.answers(session.id), api.history.context(session.id)]).then(([turns, answers, context]) => {
      if (!alive) return;
      setLines(mergeConversation(turns.ok ? turns.value : [], answers.ok ? answers.value : []));
      setNotes(context.ok ? context.value : []);
    });
    return () => {
      alive = false;
    };
  }, [session.id]);

  async function summarize() {
    setSummarizing(true);
    setSummaryError(null);
    const result = await api.history.summarize(session.id);
    setSummarizing(false);
    if (result.ok) setSummary(result.value);
    else setSummaryError("Couldn't create a summary. Check your AI settings.");
  }

  const exportLines = tab === "conversation" && lines ? conversationText(session.title, lines) : tab === "notes" && notes ? [`Context notes: ${session.title}`, "", ...notes.map((n) => `- ${n}`)] : [];
  const canExport = exportLines.length > 2 && tab !== "summary";
  const base = `${slug(session.title)}-${tab}`;

  return (
    <Dialog
      open
      onClose={onClose}
      size="xl"
      title={session.title}
      subtitle={[session.company, `${formatDate(session.started_at_ms)}, ${formatRelative(session.started_at_ms)}`, formatDuration(session.duration_ms)].filter(Boolean).join(" · ")}
      footer={
        <>
          <Button variant="danger" icon="trash" onClick={onDelete}>Delete call</Button>
          <span className="grow" />
          {canExport ? (
            <>
              <Button variant="secondary" icon="download" onClick={() => downloadText(`${base}.txt`, exportLines.join("\n"))}>Text</Button>
              <Button variant="secondary" icon="download" onClick={() => downloadPdf(`${base}.pdf`, `Aside ${tab}`, exportLines)}>PDF</Button>
            </>
          ) : null}
          <Button variant="primary" onClick={onClose}>Done</Button>
        </>
      }
    >
      <div className="hm-viewer__tabs">
        <Segmented<Tab> label="What to show" value={tab} onChange={setTab} options={[{ value: "conversation", label: "Conversation" }, { value: "notes", label: "Notes" }, { value: "summary", label: "AI summary" }]} />
      </div>

      {tab === "conversation" ? (
        lines === null ? <p className="hm-muted hm-center"><Spinner /> Loading…</p>
          : lines.length === 0 ? <p className="hm-muted hm-center">Nothing recorded.{session.segment_count === 0 ? " Transcript was off." : ""}</p>
          : (
            <div className="hm-convo selectable" aria-label="Recorded conversation">
              {lines.map((line, index) =>
                line.kind === "turn" ? (
                  <div key={index} className={`hm-msg hm-msg--${line.who}`}>
                    <span className="hm-msg__who">{line.who === "you" ? "You" : "Others"}<time>{formatClock(Math.max(0, line.at - session.started_at_ms))}</time></span>
                    <p>{line.text}</p>
                  </div>
                ) : (
                  <div key={index} className="hm-suggest">
                    <span className="hm-suggest__tag"><Icon name="sparkle" size={13} />Aside suggested</span>
                    <p>{line.text}</p>
                  </div>
                ),
              )}
            </div>
          )
      ) : null}

      {tab === "notes" ? (
        notes === null ? <p className="hm-muted hm-center"><Spinner /> Loading…</p>
          : notes.length === 0 ? <p className="hm-muted hm-center">No notes.</p>
          : <ul className="hm-notes selectable">{notes.map((note, index) => <li key={index}>{note}</li>)}</ul>
      ) : null}

      {tab === "summary" ? (
        <div className="hm-summary selectable">
          {summary ? (
            <>
              <section><h3>Overview</h3><p>{summary.paragraph}</p></section>
              <section><h3>Important notes</h3><ul>{summary.highlights.map((item, index) => <li key={index}>{item}</li>)}</ul></section>
            </>
          ) : summaryError ? <Callout tone="bad">{summaryError}</Callout> : (
            <p className="hm-muted">Get a short overview and key points.</p>
          )}
          <div><Button variant={summary ? "secondary" : "primary"} icon="sparkle" busy={summarizing} onClick={() => void summarize()}>{summary ? "Refresh summary" : "Create summary"}</Button></div>
        </div>
      ) : null}
    </Dialog>
  );
}
