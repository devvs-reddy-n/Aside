import { useEffect } from "react";
import { createStore, useStore } from "../core/store";
import { Icon } from "./Icon";

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
    ms: options.ms ?? (options.action ? 8000 : 4200),
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
  useEffect(() => {
    const timer = window.setTimeout(() => dismissToast(item.id), item.ms);
    return () => window.clearTimeout(timer);
  }, [item.id, item.ms]);
  const icon = item.tone === "ok" ? "check" : item.tone === "bad" || item.tone === "warn" ? "alert" : "info";
  return (
    <div className={`toast toast--${item.tone}`} role={item.tone === "bad" ? "alert" : "status"}>
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
    </div>
  );
}

/** Mount once near the root of each window. */
export function ToastHost() {
  const items = useStore(store, (state) => state.items);
  return (
    <div className="toast-host" aria-live="polite">
      {items.map((item) => (
        <Toast key={item.id} item={item} />
      ))}
    </div>
  );
}
