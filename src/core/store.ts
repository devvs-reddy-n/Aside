import { useRef, useSyncExternalStore } from "react";

/**
 * A tiny state holder. One store per window; screens read slices of it with
 * `useStore` and only re-draw when the slice they read actually changed.
 * No library needed: this is all Aside's screens require.
 */
export interface Store<S> {
  get: () => S;
  /** Merge a patch (or a function that returns one) into the state. */
  set: (patch: Partial<S> | ((state: S) => Partial<S>)) => void;
  /** Replace the whole state with what a function returns. */
  update: (fn: (state: S) => S) => void;
  subscribe: (listener: () => void) => () => void;
}

export function createStore<S extends object>(initial: S): Store<S> {
  let state = initial;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((listener) => listener());
  return {
    get: () => state,
    set: (patch) => {
      const next = typeof patch === "function" ? patch(state) : patch;
      state = { ...state, ...next };
      emit();
    },
    update: (fn) => {
      const next = fn(state);
      if (next === state) return;
      state = next;
      emit();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

function shallowEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  if (keysA.length !== keysB.length) return false;
  return keysA.every((key) => Object.is((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]));
}

/** Read a slice of a store. `select` must be a pure function of the state. */
export function useStore<S extends object, T>(store: Store<S>, select: (state: S) => T): T {
  const last = useRef<{ state: S; picked: T } | null>(null);
  const getSnapshot = (): T => {
    const state = store.get();
    const cached = last.current;
    if (cached && cached.state === state) return cached.picked;
    const picked = select(state);
    if (cached && shallowEqual(cached.picked, picked)) {
      last.current = { state, picked: cached.picked };
      return cached.picked;
    }
    last.current = { state, picked };
    return picked;
  };
  return useSyncExternalStore(store.subscribe, getSnapshot, getSnapshot);
}
