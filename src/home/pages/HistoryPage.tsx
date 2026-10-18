import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import * as api from "../../bridge/api";
import type { PastSessionSummary, SessionSummary, TranscriptSearchResult } from "../../bridge/types";
import { conversationText, mergeConversation, type ConversationLine } from "../../core/conversation";
import { downloadPdf, downloadText } from "../../core/downloads";
import { formatClock, formatDate, formatDuration, formatTime, plural, slug } from "../../core/format";
import { groupByDay, isShort } from "../../core/historyGroups";
import { useStore } from "../../core/store";
import { Badge, Callout, EmptyState, MetaChips, Spinner } from "../../kit/Bits";
import { Button, IconButton } from "../../kit/Button";
import { Dialog } from "../../kit/Dialog";
import { Segmented } from "../../kit/Form";
import { Icon } from "../../kit/Icon";
import { Monogram } from "../Sidebar";
import { Menu } from "../../kit/Menu";
import { RichText } from "../../kit/RichText";
import { FAST, SPRING } from "../../kit/motion";
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
  const [deleteIds, setDeleteIds] = useState<string[] | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

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
  const [showShort, setShowShort] = useState(false);
  const term = query.trim().toLowerCase();
  const matching = useMemo(() => {
    if (!term) return visible;
    return visible.filter((session) => `${session.title} ${session.company ?? ""}`.toLowerCase().includes(term));
  }, [visible, term]);
  // Accidental starts stay out of the way unless asked for (or searched for).
  const shortCount = term ? 0 : matching.filter((session) => isShort(session) && session.id !== activeId).length;
  const listed = showShort || term ? matching : matching.filter((session) => !isShort(session) || session.id === activeId);
  const groups = useMemo(() => groupByDay(listed), [listed]);
  const selected = selectedId ? visible.find((session) => session.id === selectedId) ?? null : null;
  const deletable = visible.filter((session) => session.id !== activeId && session.status !== "active");

  async function removeAll() {
    if (!deleteIds || deleting) return;
    setDeleting(true);
    setDeleteError(null);
    const result = await api.history.removeAll(deleteIds);
    setDeleting(false);
    if (!result.ok) {
      setDeleteError("Couldn't delete your history. Nothing was removed. Please try again.");
      return;
    }
    setDeleteIds(null);
    setQuery("");
    setHits([]);
    navigate("history");
    await loadHistory();
    toast(`Deleted ${plural(result.value, "call")}`, { tone: "info" });
  }

  function remove(session: PastSessionSummary) {
    if (selectedId === session.id) navigate("history");
    deleteWithUndo({
      label: `“${session.company || session.title}”`,
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
    <div className="hm-page hist">
      <header className="pg-head">
        <div className="pg-head__titles">
          <h1>History</h1>
          {visible.length > 0 ? <p className="pg-sub tabular">{plural(visible.length, "call")}</p> : null}
        </div>
        {deletable.length > 0 ? <Button variant="ghost" className="hist-delete-all" icon="trash" onClick={() => {
          setDeleteError(null);
          setDeleteIds(deletable.map((session) => session.id));
        }}>Delete all</Button> : null}
      </header>

      {error ? <Callout tone="bad" title="Couldn't load your history">{error}</Callout> : null}

      {visible.length > 0 ? (
        <label className="hm-search">
          <Icon name="search" size={15} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search calls and what was said" aria-label="Search history" onKeyDown={(event) => { if (event.key === "Escape") setQuery(""); }} />
          {query ? <IconButton icon="x" label="Clear history search" size="sm" onClick={() => setQuery("")} /> : null}
          {searching ? <Spinner size={14} /> : null}
        </label>
      ) : null}

      {loaded && visible.length === 0 ? (
        <EmptyState icon="history" title="No calls yet" action={<Button variant="primary" onClick={() => navigate("copilots")}>Go to Copilots</Button>}>
          Finished sessions appear here, with their transcript when it was saved.
        </EmptyState>
      ) : (
        <div className="pg-panel">
          {hits.length > 0 ? (
            <section aria-label="Found in conversations" className="pg-card hm-hits">
              <header className="pg-card__head"><Icon name="search" size={15} /><h2>Found in conversations</h2></header>
              <ul className="hist-list">
                {hits.slice(0, 20).map((hit, index) => (
                  <li key={`${hit.session_id}-${hit.start_ms}-${index}`}>
                    <a className="hm-hit" href={hrefFor("history", hit.session_id)}>
                      <span className="hm-hit__who">{hit.speaker === "you" ? "You" : "Them"}</span>
                      <Highlight text={hit.text} term={query.trim()} />
                      <small>{hit.session_title}</small>
                    </a>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {groups.map((group) => (
            <section key={group.key} className="pg-card hist-day" aria-label={group.label}>
              <header className="pg-card__head"><Icon name="calendar" size={15} /><h2>{group.label}</h2></header>
              <ul className="hist-list">
                <AnimatePresence initial={false}>
                  {group.items.map((session) => (
                    <CallRow key={session.id} session={session} live={session.id === activeId} onDelete={() => remove(session)} />
                  ))}
                </AnimatePresence>
              </ul>
            </section>
          ))}

          {shortCount > 0 ? (
            <p className="hist-short">
              {plural(shortCount, "short session")} hidden · <button type="button" onClick={() => setShowShort((v) => !v)}>{showShort ? "Hide" : "Show"}</button>
            </p>
          ) : null}
          {loaded && matching.length === 0 && visible.length > 0 && hits.length === 0 && !searching ? <p className="lib-none">No call matches “{query}”.</p> : null}
        </div>
      )}

      {selected ? <SessionSheet session={selected} onClose={() => navigate("history")} /> : null}
      <Dialog open={deleteIds !== null} onClose={() => { if (!deleting) setDeleteIds(null); }}
        title="Delete all history?" size="sm" hideClose={deleting}
        footer={<><Button variant="ghost" disabled={deleting} onClick={() => setDeleteIds(null)}>Cancel</Button>
          <Button variant="danger" className="btn--end-quit" busy={deleting} onClick={() => void removeAll()}>Delete all</Button></>}>
        <p>This permanently deletes {plural(deleteIds?.length ?? 0, "saved call")} and their transcripts and answers. This cannot be undone.</p>
        <p>Your Library files, copilots and any running call stay in place.</p>
        {deleteError ? <Callout tone="bad" title="History wasn't deleted">{deleteError}</Callout> : null}
      </Dialog>
    </div>
  );
}

function CallRow({ session, live, onDelete }: { session: PastSessionSummary; live: boolean; onDelete: () => void }) {
  const name = session.company || session.title;
  const role = session.company && session.title !== session.company ? session.title : null;
  return (
    <motion.li
      layout="position"
      className="hist-row"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, height: 0 }}
      transition={{ duration: FAST }}
    >
      <a className="hist-row__main" href={hrefFor("history", session.id)}>
        <motion.span layoutId={`call-mono-${session.id}`} transition={SPRING} className="hist-row__mono"><Monogram name={name} size={34} /></motion.span>
        <span className="hist-row__text">
          <b>{name}</b>
          {role ? <span className="hist-row__role">{role}</span> : null}
          <MetaChips items={[
            { name: "Time", label: formatTime(session.started_at_ms) },
            { name: "Duration", label: live ? null : formatDuration(session.duration_ms) },
            { icon: "chat", label: !live && session.segment_count > 0 ? "Transcript" : null },
          ]} />
        </span>
        {live ? <Badge tone="ok">Live now</Badge> : null}
      </a>
      {live ? <Button size="sm" variant="secondary" onClick={() => void returnToCall()}>Return to call</Button> : null}
      <div className="hist-row__menu">
        <Menu
          trigger={<IconButton icon="more" label={`More for ${name}`} size="sm" />}
          items={[
            { label: "Open", icon: "eye", onSelect: () => navigate("history", session.id) },
            "separator",
            { label: "Delete", icon: "trash", danger: true, disabled: live, onSelect: onDelete },
          ]}
        />
      </div>
    </motion.li>
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

function SessionSheet({ session, onClose }: { session: PastSessionSummary; onClose: () => void }) {
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
      title={session.company || session.title}
      icon={<motion.span layoutId={`call-mono-${session.id}`} transition={SPRING} className="hist-row__mono"><Monogram name={session.company || session.title} size={40} /></motion.span>}
      subtitle={<MetaChips items={[
        { icon: "briefcase", name: "Role", label: session.company && session.title !== session.company ? session.title : null },
        { icon: "clock", name: "Date", label: `${formatDate(session.started_at_ms)}, ${formatTime(session.started_at_ms)}` },
        { name: "Duration", label: formatDuration(session.duration_ms) },
      ]} />}
      footer={
        <>
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
                    <span className="hm-msg__who">{line.who === "you" ? "You" : "Them"}<time>{formatClock(Math.max(0, line.at - session.started_at_ms))}</time></span>
                    <p>{line.text}</p>
                  </div>
                ) : (
                  <div key={index} className="hm-suggest">
                    <span className="hm-suggest__tag"><Icon name="sparkle" size={13} />Aside suggested</span>
                    <RichText text={line.text} lines={200} />
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
