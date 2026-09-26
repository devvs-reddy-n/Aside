import { createContext, useContext, type ReactNode } from "react";
import { useStore } from "../core/store";
import type { LiveController, LiveUi } from "./controller";
import type { LiveState } from "./machine";

const Context = createContext<LiveController | null>(null);

export function LiveProvider({ controller, children }: { controller: LiveController; children: ReactNode }) {
  return <Context.Provider value={controller}>{children}</Context.Provider>;
}

export function useController(): LiveController {
  const controller = useContext(Context);
  if (!controller) throw new Error("useController must be used inside <LiveProvider>");
  return controller;
}

export function useLive<T>(select: (state: LiveState) => T): T {
  return useStore(useController().live, select);
}

export function useLiveUi<T>(select: (state: LiveUi) => T): T {
  return useStore(useController().ui, select);
}
