import { LayoutGroup, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { CopilotSummary } from "../bridge/types";
import { formatRelative } from "../core/format";
import { useStore } from "../core/store";
import { monogram, tintHue } from "../core/text";
import { IconButton } from "../kit/Button";
import { BrandMark } from "../kit/BrandMark";
import { Icon, type IconName } from "../kit/Icon";
import { SPRING } from "../kit/motion";
import { Tip } from "../kit/Tooltip";
import { copilotsStore, loadCopilots } from "./data";
import { hrefFor, useRoute, type PageId } from "./router";
import { StatusPill } from "./StatusPill";

const KEY = "aside-sidebar-v2";
const MIN = 220;
const MAX = 300;
const DEFAULT = 224;

const PLACES: Array<{ page: PageId; param?: string; label: string; icon: IconName }> = [
  { page: "copilots", param: "new", label: "New Copilot", icon: "plus" },
  { page: "library", label: "Library", icon: "library" },
  { page: "history", label: "History", icon: "history" },
  { page: "settings", label: "Settings", icon: "gear" },
];

function readLayout(): { width: number; collapsed: boolean } {
  try {
    const raw = JSON.parse(window.localStorage.getItem(KEY) ?? "null") as { width?: number; collapsed?: boolean } | null;
    return { width: Math.min(MAX, Math.max(MIN, raw?.width ?? DEFAULT)), collapsed: raw?.collapsed === true };
  } catch {
    return { width: DEFAULT, collapsed: false };
  }
}

function saveLayout(layout: { width: number; collapsed: boolean }) {
  try {
    window.localStorage.setItem(KEY, JSON.stringify(layout));
  } catch {
    // Not remembered; still applied for now.
  }
}

/** A small rounded square with the company's initials, tinted by its name. */
export function Monogram({ name, size = 24 }: { name: string; size?: number }) {
  return (
    <span className="mono" style={{ "--h": tintHue(name), width: size, height: size, fontSize: size * 0.42 } as CSSProperties} aria-hidden="true">
      {monogram(name)}
    </span>
  );
}

/** The selection highlight glides between sidebar rows. */
function Highlight({ group = "copilot" }: { group?: "navigation" | "copilot" }) {
  return <motion.span layoutId={`sb-selection-${group}`} className="sb-item__bg" transition={SPRING} />;
}

function SidebarLink({ href, current, rail, tip, children }: { href: string; current: boolean; rail: boolean; tip: string; children: ReactNode }) {
  const link = (
    <a className="sb-item" href={href} aria-current={current ? "page" : undefined} aria-label={rail ? tip : undefined}>
      {current ? <Highlight group="navigation" /> : null}
      {children}
    </a>
  );
  return rail ? <Tip label={tip} side="right">{link}</Tip> : link;
}

function CopilotRow({ copilot, current, rail }: { copilot: CopilotSummary; current: boolean; rail: boolean }) {
  const title = copilot.company || copilot.name;
  const tip = `${title}${copilot.target_role ? ` · ${copilot.target_role}` : ""}\nUpdated ${formatRelative(copilot.updated_at_ms).toLowerCase()}`;
  const link = (
    <a className="sb-item sb-item--copilot" href={hrefFor("copilots", copilot.id)} aria-current={current ? "page" : undefined} aria-label={rail ? title : undefined}>
      {current ? <Highlight /> : null}
      <Monogram name={title} size={30} />
      {rail ? null : (
        <span className="sb-item__text">
          <b>{title}</b>
          {copilot.target_role ? <small>{copilot.target_role}</small> : null}
        </span>
      )}
    </a>
  );
  return <Tip label={tip} side="right">{link}</Tip>;
}

export function Sidebar() {
  const route = useRoute();
  const { items, hiddenIds } = useStore(copilotsStore, (state) => state);
  const [layout, setLayout] = useState(readLayout);
  const [query, setQuery] = useState("");
  const dragging = useRef(false);

  useEffect(() => {
    void loadCopilots();
  }, []);

  const copilots = useMemo(() => {
    const term = layout.collapsed ? "" : query.trim().toLowerCase();
    return items.filter((copilot) => copilot.kind !== "screen" && !hiddenIds.includes(copilot.id) && (!term || `${copilot.company ?? ""} ${copilot.name} ${copilot.target_role ?? ""}`.toLowerCase().includes(term)));
  }, [items, hiddenIds, query, layout.collapsed]);
  const total = items.filter((copilot) => copilot.kind !== "screen" && !hiddenIds.includes(copilot.id)).length;
  const selectedCopilot = route.page === "copilots" && route.param !== "new" ? route.param ?? copilots[0]?.id ?? null : null;
  const rail = layout.collapsed;

  function startResize(event: React.PointerEvent) {
    event.preventDefault();
    dragging.current = true;
    const startX = event.clientX;
    const startWidth = layout.width;
    const move = (e: PointerEvent) => {
      if (!dragging.current) return;
      setLayout((current) => ({ ...current, width: Math.min(MAX, Math.max(MIN, startWidth + e.clientX - startX)) }));
    };
    const up = () => {
      dragging.current = false;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setLayout((current) => {
        saveLayout(current);
        return current;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  return (
    <nav className="sb" data-rail={rail || undefined} style={{ width: rail ? 64 : layout.width }} aria-label="Aside">
      <div className="sb__top" data-tauri-drag-region>
        <a className="sb__brand" href={hrefFor("copilots")} aria-label="Aside home">
          <span className="sb__logo" aria-hidden="true"><BrandMark /></span>
          {rail ? null : <span className="sb__name">Aside</span>}
        </a>
        <IconButton
          icon="sidebar"
          label={rail ? "Show sidebar" : "Hide sidebar"}
          size="sm"
          onClick={() => setLayout((current) => {
            const next = { ...current, collapsed: !current.collapsed };
            saveLayout(next);
            return next;
          })}
        />
      </div>

      <LayoutGroup id="sidebar">
        <ul className="sb__list sb__places">
          {PLACES.map((place) => (
            <li key={`${place.page}/${place.param ?? ""}`}>
              <SidebarLink href={hrefFor(place.page, place.param)} current={route.page === place.page && (!place.param || route.param === place.param)} rail={rail} tip={place.label}>
                <Icon name={place.icon} size={18} />{rail ? null : <span className="sb-item__label">{place.label}</span>}
              </SidebarLink>
            </li>
          ))}
        </ul>
        <div className="sb__group">
          {rail ? null : (
            <div className="sb__head">
              <span>Your copilots</span>
            </div>
          )}
          {!rail && total > 6 ? (
            <label className="sb__search">
              <Icon name="search" size={14} />
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter" aria-label="Filter copilots" />
            </label>
          ) : null}
          <ul className="sb__list">
            {copilots.map((copilot) => (
              <li key={copilot.id}>
                <CopilotRow copilot={copilot} current={selectedCopilot === copilot.id} rail={rail} />
              </li>
            ))}
            {!rail && total > 0 && copilots.length === 0 ? (
              <li className="sb__empty" role="status">No matching copilots. <button type="button" onClick={() => setQuery("")}>Clear filter</button></li>
            ) : null}
            {!rail && total === 0 ? <li className="sb__empty">None yet</li> : null}
          </ul>
        </div>

        <div className="sb__spacer" />

      </LayoutGroup>

      <div className="sb__foot">
        <StatusPill compact={rail} />
      </div>

      {rail ? null : <div
        className="sb__resize" role="separator" aria-orientation="vertical" aria-label="Resize sidebar"
        tabIndex={0} aria-valuemin={MIN} aria-valuemax={MAX} aria-valuenow={layout.width}
        aria-valuetext={`${layout.width} pixels wide`} onPointerDown={startResize}
        onKeyDown={(event) => {
          if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
          event.preventDefault();
          const key = event.key;
          setLayout((current) => {
            const width = key === "Home" ? MIN : key === "End" ? MAX : Math.min(MAX, Math.max(MIN, current.width + (key === "ArrowRight" ? 10 : -10)));
            const next = { ...current, width };
            saveLayout(next);
            return next;
          });
        }}
      />}
    </nav>
  );
}
