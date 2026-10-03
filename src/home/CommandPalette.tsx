import * as D from "@radix-ui/react-dialog";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { formatDate } from "../core/format";
import { useStore } from "../core/store";
import { displayFileName } from "../core/text";
import { Icon, type IconName } from "../kit/Icon";
import { Keys } from "../kit/Keycap";
import { BASE, EASE_OUT, FAST } from "../kit/motion";
import { onMessage, send } from "./bus";
import { copilotsStore, historyStore, libraryStore, loadHistory, loadLibrary } from "./data";
import { navigate, useRoute } from "./router";

interface Command {
  id: string;
  group: "Actions" | "Copilots" | "Library" | "History" | "Settings";
  label: string;
  detail?: string;
  icon: IconName;
  run: () => void;
}

const SETTINGS: Array<[string, string]> = [
  ["speech", "Speech"],
  ["ai", "AI model"],
  ["rules", "Answer rules"],
  ["appearance", "Appearance"],
  ["shortcuts", "Shortcuts"],
  ["general", "Screen sharing"],
];

/** Everything in one searchable list, opened from the sidebar search button. */
export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLUListElement>(null);
  const route = useRoute();
  const copilots = useStore(copilotsStore, (state) => state.items.filter((item) => !state.hiddenIds.includes(item.id)));
  const docs = useStore(libraryStore, (state) => state.docs);
  const sessions = useStore(historyStore, (state) => state.sessions);

  useEffect(() => onMessage("open-palette", () => setOpen(true)), []);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setIndex(0);
    void loadLibrary();
    void loadHistory();
  }, [open]);

  const commands = useMemo<Command[]>(() => {
    const onCopilot = route.page === "copilots" && route.param !== "new";
    const all: Command[] = [
      { id: "new", group: "Actions", label: "New copilot", icon: "plus", run: () => navigate("copilots", "new") },
      ...(onCopilot ? [{ id: "start", group: "Actions" as const, label: "Start session", icon: "play" as IconName, run: () => send("start-session") }] : []),
      { id: "add", group: "Actions", label: "Add files to Library", icon: "upload", run: () => { navigate("library"); window.setTimeout(() => send("add-files"), 50); } },
      ...copilots.map((copilot): Command => ({ id: `c-${copilot.id}`, group: "Copilots", label: copilot.company || copilot.name, detail: copilot.target_role ?? undefined, icon: "spark", run: () => navigate("copilots", copilot.id) })),
      ...docs.map((doc): Command => ({ id: `d-${doc.id}`, group: "Library", label: displayFileName(doc.file_name), detail: doc.doc_type === "resume" ? "Resume" : "Document", icon: doc.doc_type === "resume" ? "doc" : "note", run: () => navigate("library", doc.doc_type === "resume" ? "resumes" : "documents") })),
      ...sessions.map((session): Command => ({ id: `s-${session.id}`, group: "History", label: session.company || session.title, detail: `${session.title} · ${formatDate(session.started_at_ms)}`, icon: "history", run: () => navigate("history", session.id) })),
      ...SETTINGS.map(([section, label]): Command => ({ id: `set-${section}`, group: "Settings", label, icon: "gear", run: () => navigate("settings", section) })),
    ];
    const term = query.trim().toLowerCase();
    if (!term) return all.filter((command) => command.group !== "Library" && command.group !== "History").concat(all.filter((c) => c.group === "History").slice(0, 3));
    return all.filter((command) => `${command.label} ${command.detail ?? ""} ${command.group}`.toLowerCase().includes(term)).slice(0, 40);
  }, [copilots, docs, sessions, query, route]);

  useEffect(() => setIndex(0), [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  function choose(command: Command | undefined) {
    if (!command) return;
    setOpen(false);
    command.run();
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setIndex((current) => Math.min(commands.length - 1, current + 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setIndex((current) => Math.max(0, current - 1));
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(commands[index]);
    }
  }

  let lastGroup = "";
  return (
    <D.Root open={open} onOpenChange={setOpen}>
      <AnimatePresence>
        {open ? (
          <D.Portal forceMount>
            <D.Overlay asChild forceMount>
              <motion.div className="palette-backdrop" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: FAST }} />
            </D.Overlay>
            <D.Content asChild forceMount aria-describedby={undefined}>
              <motion.div className="palette" initial={{ opacity: 0, scale: 0.97 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: 0.97 }} transition={{ duration: BASE, ease: EASE_OUT }}>
                <D.Title className="sr-only">Command palette</D.Title>
                <div className="palette__search">
                  <Icon name="search" size={16} />
                  <input autoFocus value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={onKeyDown} placeholder="Search copilots, files, calls, settings…" aria-label="Search" aria-activedescendant={commands[index] ? `cmd-${commands[index].id}` : undefined} />
                  <Keys combo="esc" />
                </div>
                <ul ref={list} className="palette__list" role="listbox" aria-label="Results">
                  {commands.length === 0 ? <li className="palette__empty">No results</li> : null}
                  {commands.map((command, i) => {
                    const header = command.group !== lastGroup ? command.group : null;
                    lastGroup = command.group;
                    return (
                      <li key={command.id} role="presentation">
                        {header ? <p className="palette__group">{header}</p> : null}
                        <button
                          type="button"
                          id={`cmd-${command.id}`}
                          role="option"
                          aria-selected={i === index}
                          data-index={i}
                          className="palette__item"
                          onMouseMove={() => setIndex(i)}
                          onClick={() => choose(command)}
                        >
                          <Icon name={command.icon} size={15} />
                          <span className="palette__label">{command.label}</span>
                          {command.detail ? <span className="palette__detail">{command.detail}</span> : null}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              </motion.div>
            </D.Content>
          </D.Portal>
        ) : null}
      </AnimatePresence>
    </D.Root>
  );
}
