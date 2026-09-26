import { useSyncExternalStore } from "react";

export type PageId = "copilots" | "resumes" | "documents" | "history" | "settings";

export interface Route {
  page: PageId;
  /** A selected item (copilot id, session id) or a settings section. */
  param: string | null;
}

const PAGES: PageId[] = ["copilots", "resumes", "documents", "history", "settings"];

export function parseHash(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  const page = PAGES.find((candidate) => candidate === parts[0]) ?? "copilots";
  return { page, param: parts[1] ? decodeURIComponent(parts[1]) : null };
}

export function hrefFor(page: PageId, param?: string | null): string {
  return `#/${page}${param ? `/${encodeURIComponent(param)}` : ""}`;
}

export function navigate(page: PageId, param?: string | null): void {
  window.location.hash = hrefFor(page, param);
}

let cached: { hash: string; route: Route } | null = null;

function snapshot(): Route {
  const hash = window.location.hash;
  if (!cached || cached.hash !== hash) cached = { hash, route: parseHash(hash) };
  return cached.route;
}

function subscribe(listener: () => void): () => void {
  window.addEventListener("hashchange", listener);
  return () => window.removeEventListener("hashchange", listener);
}

export function useRoute(): Route {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
