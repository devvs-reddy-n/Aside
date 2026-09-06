import { useEffect, useRef, useState } from "react";
import { invoke } from "../platform/invoke";
import { useTauriEvent } from "../platform/events";
import { isTauriRuntime } from "../platform/runtime";
import type { TimingSummary, WindowPreview } from "../platform/types";

/** The Rust event name from `llm::timing::EVENT_ANSWER_TIMING`, fired once
 *  after every answer with a `TimingSummary` payload. */
const EVENT_ANSWER_TIMING = "answer-timing";

/** How many recent presses to keep in memory. A rolling window, not a
 *  saved history — this is deliberately in-memory only, matching the
 *  Memory layer's own "resets when Aside restarts" behaviour. */
const MAX_ROWS = 30;

export interface PressTimingRow {
  key: string;
  /** Display order, oldest press is 1, counting up for the running
   *  session — not the same counter as `window_memory::PressLog`'s own
   *  `press_number`, which this screen never receives for live rows. */
  pressNumber: number;
  ttftMs: number | null;
  totalMs: number | null;
  /** Real character count of the material sent with this press
   *  (`TimingSummary.context_chars`), or `null` for a row seeded from an
   *  older `PressLog` that never carried this figure. */
  contextChars: number | null;
  inputTokens: number | null;
  cachedInputTokens: number | null;
  /** False for the one row (if any) seeded from `get_window_preview`'s
   *  `last_log` on mount — that record has no reuse figures at all, only
   *  timing, so this screen must say so rather than guessing. */
  hasReuseData: boolean;
}

export interface AnswerTimingState {
  tauri: boolean;
  /** Newest first. */
  rows: PressTimingRow[];
  averageTtftMs: number | null;
  averageReusePercent: number | null;
}

function rowFromSummary(summary: TimingSummary, pressNumber: number): PressTimingRow {
  return {
    key: `live-${pressNumber}-${summary.total_ms ?? summary.ttft_ms ?? Math.random()}`,
    pressNumber,
    ttftMs: summary.ttft_ms,
    totalMs: summary.total_ms,
    contextChars: summary.context_chars,
    inputTokens: summary.input_tokens,
    cachedInputTokens: summary.cached_input_tokens,
    hasReuseData: true,
  };
}

/**
 * A rolling record of what happened on each recent Command+Enter press:
 * how long it took, and whether the material that is supposed to stay the
 * same (resume, job description, style notes) actually got the cheaper
 * repeat rate. Built from the real `answer-timing` event fired after every
 * answer (`llm::timing::summarize`), never an estimate.
 *
 * On first mount, one row is seeded from `get_window_preview`'s `last_log`
 * (a `PressLog`) so the screen is not empty on the very first visit after
 * a press already happened earlier in the session — that seeded row has
 * no reuse figures, since `PressLog` never carried them, and is marked
 * `hasReuseData: false` so the row can say so plainly instead of showing
 * a fabricated number.
 */
export function useAnswerTiming(): AnswerTimingState {
  const tauri = isTauriRuntime();
  const [rows, setRows] = useState<PressTimingRow[]>([]);
  const counterRef = useRef(0);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let cancelled = false;
    (async () => {
      const result = await invoke<WindowPreview>("get_window_preview");
      if (cancelled || !result.ok) return;
      const log = result.value.last_log;
      if (!log) return;
      setRows((prev) => {
        // A live event may have already arrived while this request was in
        // flight — never overwrite real data with a seeded estimate.
        if (prev.length > 0) return prev;
        counterRef.current += 1;
        return [
          {
            key: `seed-${log.press_number}`,
            pressNumber: counterRef.current,
            ttftMs: log.ttft_ms,
            totalMs: log.total_ms,
            contextChars: null,
            inputTokens: null,
            cachedInputTokens: null,
            hasReuseData: false,
          },
        ];
      });
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useTauriEvent<TimingSummary>(EVENT_ANSWER_TIMING, (payload) => {
    counterRef.current += 1;
    const row = rowFromSummary(payload, counterRef.current);
    setRows((prev) => [row, ...prev].slice(0, MAX_ROWS));
  });

  const withTtft = rows.filter((row) => row.ttftMs != null);
  const averageTtftMs =
    withTtft.length > 0
      ? Math.round(withTtft.reduce((sum, row) => sum + (row.ttftMs ?? 0), 0) / withTtft.length)
      : null;

  const withReuse = rows.filter(
    (row) => row.hasReuseData && row.inputTokens != null && row.inputTokens > 0 && row.cachedInputTokens != null,
  );
  const averageReusePercent =
    withReuse.length > 0
      ? Math.round(
          (withReuse.reduce((sum, row) => sum + (row.cachedInputTokens as number) / (row.inputTokens as number), 0) /
            withReuse.length) *
            100,
        )
      : null;

  return { tauri, rows, averageTtftMs, averageReusePercent };
}
