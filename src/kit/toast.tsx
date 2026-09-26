import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { createStore, useStore } from "../core/store";
import { Icon } from "./Icon";
import { BASE, EASE_OUT } from "./motion";

interface ToastItem {
  id: number;
  message: string;
  tone: "info" | "ok" | "warn" | "bad";
  action?: { label: string; run: () => void };
  /** Runs when the toast goes away without its action being pressed. */
  onDone?: () => void;
  ms: number;
}

const store = createStore<{ items: ToastItem[] }>({ items: [] });
let counter = 0;

export function toast(
  message: string,
  options: { tone?: ToastItem["tone"]; action?: ToastItem["action"]; onDone?: () => void; ms?: number } = {},
): number {
  const id = ++counter;
  const item: ToastItem = {
    id,
    message,
    tone: options.tone ?? "info",
    action: options.action,
    onDone: options.onDone,
    ms: options.ms ?? 5000,
  };
  store.set((state) => ({ items: [...state.items.slice(-3), item] }));
  return id;
}

export function dismissToast(id: number, viaAction = false): void {
  const item = store.get().items.find((entry) => entry.id === id);
  if (!item) return;
  store.set((state) => ({ items: state.items.filter((entry) => entry.id !== id) }));
  if (!viaAction) item.onDone?.();
}

function Toast({ item }: { item: ToastItem }) {
  // The countdown pauses while the pointer is over the toast, so it never vanishes mid-read.
  const [paused, setPaused] = useState(false);
  const left = useRef(item.ms);
  useEffect(() => {
    if (paused) return;
    const started = Date.now();
    const timer = window.setTimeout(() => dismissToast(item.id), left.current);
    return () => {
      window.clearTimeout(timer);
      left.current = Math.max(600, left.current - (Date.now() - started));
    };
  }, [item.id, paused]);
  const icon = item.tone === "ok" ? "checkCircle" : item.tone === "bad" || item.tone === "warn" ? "alertCircle" : "info";
  return (
    <motion.div
      layout
      className={`toast toast--${item.tone}`}
      role={item.tone === "bad" ? "alert" : "status"}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 8 }}
      transition={{ duration: BASE, ease: EASE_OUT }}
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
    >
      <Icon name={icon} size={16} />
      <span className="toast__text">{item.message}</span>
      {item.action ? (
        <button
          type="button"
          className="toast__action"
          onClick={() => {
            item.action?.run();
            dismissToast(item.id, true);
          }}
        >
          {item.action.label}
        </button>
      ) : null}
      <button type="button" className="toast__close" aria-label="Dismiss" onClick={() => dismissToast(item.id)}>
        <Icon name="x" size={14} />
      </button>
    </motion.div>
  );
}

/** Mount once near the root of each window. */
export function ToastHost() {
  const items = useStore(store, (state) => state.items);
  return (
    <div className="toast-host" aria-live="polite">
      <AnimatePresence initial={false}>
        {items.map((item) => <Toast key={item.id} item={item} />)}
      </AnimatePresence>
    </div>
  );
}
