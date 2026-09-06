/**
 * Aside's icon system (DESIGN.md §7).
 *
 * One set, one grid, one stroke weight. **No emoji anywhere in the product
 * UI** — beyond carrying another vendor's art direction and rendering
 * inconsistently, emoji-as-icons was the direct cause of raw `\u{1F4C4}`
 * escape sequences appearing as visible text in Output Settings.
 *
 * Rules every glyph here follows:
 *   - `16×16` viewBox, `1.5` stroke, `currentColor`, round caps and joins.
 *   - No fills unless the glyph is genuinely solid (only `dot` is).
 *   - Inherits text colour and font size context; size is set by the caller.
 */

import type { ReactElement } from "react";

export type IconName =
  | "mark"
  | "settings"
  | "close"
  | "minimize"
  | "document"
  | "note"
  | "globe"
  | "spark"
  | "chevronDown"
  | "chevronLeft"
  | "chevronRight"
  | "arrowRight"
  | "check"
  | "plus"
  | "sliders"
  | "model"
  | "mic"
  | "speaker"
  | "waveform"
  | "camera"
  | "message"
  | "eye"
  | "eyeOff"
  | "more"
  | "copy"
  | "trash"
  | "code"
  | "key"
  | "info"
  | "dot"
  | "list"
  | "layers"
  | "search"
  | "opacity"
  | "download";

/** Every path is drawn on the same 16×16 grid at 1.5 stroke. */
const PATHS: Record<IconName, ReactElement> = {
  // The mark: a line reduction of the "Aside Minimalist Logo Redesign" the
  // owner made in Google Stitch (2026-08-31, project "macOS Theme
  // Analyzer") — a single open stroke traced tip → apex → down the near-
  // vertical right edge → back up to an inner notch, matching the
  // wordmark's own arrow glyph instead of the earlier Spatial Glass
  // ribbon "A". Replaces the OS-level icon (`src-tauri/icons/`) too, so
  // both draw from the same shape again.
  mark: <path d="M2 12.6 11.9 2 11.9 13.8 8.9 8.3" />,
  settings: (
    <>
      <circle cx="8" cy="8" r="2.25" />
      <path d="M8 1.5v1.6M8 12.9v1.6M14.5 8h-1.6M3.1 8H1.5M12.6 3.4l-1.1 1.1M4.5 11.5l-1.1 1.1M12.6 12.6l-1.1-1.1M4.5 4.5 3.4 3.4" />
    </>
  ),
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  minimize: <path d="M3.5 8h9" />,
  document: (
    <>
      <path d="M9 1.8H4.6a1.1 1.1 0 0 0-1.1 1.1v10.2a1.1 1.1 0 0 0 1.1 1.1h6.8a1.1 1.1 0 0 0 1.1-1.1V5.2z" />
      <path d="M9 1.8v3.4h3.5" />
    </>
  ),
  note: (
    <>
      <path d="M11.4 2.3a1.4 1.4 0 0 1 2 2L6.1 11.6l-2.7.7.7-2.7z" />
      <path d="M10.2 3.5l2 2" />
    </>
  ),
  globe: (
    <>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M1.8 8h12.4M8 1.8c1.6 1.8 2.4 3.9 2.4 6.2S9.6 12.4 8 14.2C6.4 12.4 5.6 10.3 5.6 8S6.4 3.6 8 1.8z" />
    </>
  ),
  spark: <path d="M8 2l1.4 3.9L13.3 7.3 9.4 8.7 8 12.6 6.6 8.7 2.7 7.3l3.9-1.4z" />,
  chevronDown: <path d="M4 6.5 8 10.5l4-4" />,
  chevronLeft: <path d="M9.5 4 5.5 8l4 4" />,
  chevronRight: <path d="M6.5 4l4 4-4 4" />,
  arrowRight: <path d="M3 8h10M9 4l4 4-4 4" />,
  check: <path d="M3.2 8.4 6.3 11.5l6.5-7" />,
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  sliders: (
    <>
      <path d="M2.5 4.5h11M2.5 11.5h11" />
      <circle cx="6" cy="4.5" r="1.6" />
      <circle cx="10.5" cy="11.5" r="1.6" />
    </>
  ),
  model: (
    <>
      <rect x="4.2" y="4.2" width="7.6" height="7.6" rx="1.4" />
      <path d="M6.4 1.8v2.4M9.6 1.8v2.4M6.4 11.8v2.4M9.6 11.8v2.4M1.8 6.4h2.4M1.8 9.6h2.4M11.8 6.4h2.4M11.8 9.6h2.4" />
    </>
  ),
  mic: (
    <>
      <rect x="6" y="1.8" width="4" height="7.4" rx="2" />
      <path d="M3.6 7.4a4.4 4.4 0 0 0 8.8 0M8 11.8v2.4" />
    </>
  ),
  speaker: (
    <>
      <path d="M8.4 2.6 4.9 5.5H2.4v5h2.5l3.5 2.9z" />
      <path d="M11.2 5.8a3.1 3.1 0 0 1 0 4.4M13.3 3.7a6 6 0 0 1 0 8.6" />
    </>
  ),
  waveform: <path d="M2.5 6.5v3M5.5 3.5v9M8.5 5v6M11.5 2.5v11M14.5 6.5v3" />,
  camera: (
    <>
      <path d="M2 5.2a1.2 1.2 0 0 1 1.2-1.2h1.9L6.3 2.3h3.4L11 4h1.8A1.2 1.2 0 0 1 14 5.2v6.4a1.2 1.2 0 0 1-1.2 1.2H3.2A1.2 1.2 0 0 1 2 11.6z" />
      <circle cx="8" cy="8.3" r="2.5" />
    </>
  ),
  message: <path d="M13.6 8.9a1.5 1.5 0 0 1-1.5 1.5H5.4L2.4 13.4V4a1.5 1.5 0 0 1 1.5-1.5h8.2A1.5 1.5 0 0 1 13.6 4z" />,
  eye: (
    <>
      <path d="M1.4 8C2.2 5.8 4.6 3.6 8 3.6S13.8 5.8 14.6 8C13.8 10.2 11.4 12.4 8 12.4S2.2 10.2 1.4 8z" />
      <circle cx="8" cy="8" r="2.2" />
    </>
  ),
  eyeOff: (
    <>
      <path d="M6.4 3.7a6.6 6.6 0 0 1 1.6-.2c3.6 0 6 3 6.6 4.5a9.4 9.4 0 0 1-2 2.7M4.2 4.9C2.7 5.9 1.7 7.3 1.4 8c.6 1.5 3 4.5 6.6 4.5 1.2 0 2.2-.3 3.1-.8" />
      <path d="M2 2l12 12" />
    </>
  ),
  more: (
    <>
      <circle cx="3.6" cy="8" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="12.4" cy="8" r="1.05" fill="currentColor" stroke="none" />
    </>
  ),
  copy: (
    <>
      <rect x="5.6" y="5.6" width="8.2" height="8.2" rx="1.3" />
      <path d="M10.4 5.6V3.5a1.3 1.3 0 0 0-1.3-1.3H3.5a1.3 1.3 0 0 0-1.3 1.3v5.6a1.3 1.3 0 0 0 1.3 1.3h2.1" />
    </>
  ),
  trash: (
    <>
      <path d="M2.6 4.2h10.8M6.3 4.2V3a1 1 0 0 1 1-1h1.4a1 1 0 0 1 1 1v1.2" />
      <path d="M4 4.2v8.6a1.2 1.2 0 0 0 1.2 1.2h5.6a1.2 1.2 0 0 0 1.2-1.2V4.2" />
    </>
  ),
  code: <path d="M5.4 4.2 1.8 8l3.6 3.8M10.6 4.2 14.2 8l-3.6 3.8" />,
  key: (
    <>
      <circle cx="5.4" cy="5.4" r="3.1" />
      <path d="M7.7 7.7 14 14M11.4 11.4l1.4-1.4M9.6 9.6 11 8.2" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 7.4v3.4" />
      <circle cx="8" cy="5.3" r="0.75" fill="currentColor" stroke="none" />
    </>
  ),
  dot: <circle cx="8" cy="8" r="3.4" fill="currentColor" stroke="none" />,
  list: (
    <>
      <path d="M6 4.2h8M6 8h8M6 11.8h8" />
      <circle cx="2.6" cy="4.2" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="2.6" cy="8" r="0.9" fill="currentColor" stroke="none" />
      <circle cx="2.6" cy="11.8" r="0.9" fill="currentColor" stroke="none" />
    </>
  ),
  layers: (
    <>
      <path d="M8 2.2 2 5.8l6 3.6 6-3.6z" />
      <path d="M2 8.8l6 3.6 6-3.6M2 11.8l6 3.6 6-3.6" />
    </>
  ),
  search: (
    <>
      <circle cx="7" cy="7" r="4.6" />
      <path d="M10.3 10.3 14 14" />
    </>
  ),
  // A drop — transparency as "how much shows through."
  opacity: <path d="M8 1.8c2.3 2.9 4.6 5.9 4.6 8.4a4.6 4.6 0 0 1-9.2 0c0-2.5 2.3-5.5 4.6-8.4z" />,
  download: (
    <>
      <path d="M8 1.8v8.4M4.6 7.2 8 10.6l3.4-3.4" />
      <path d="M2.4 12.2v.9a1.1 1.1 0 0 0 1.1 1.1h9a1.1 1.1 0 0 0 1.1-1.1v-.9" />
    </>
  ),
};

export interface IconProps {
  name: IconName;
  /** Pixel size of the square glyph. Defaults to 16 (the design grid). */
  size?: number;
  className?: string;
  /** Set only when the icon is the element's sole meaning; otherwise the
   *  icon is decorative and the adjacent text carries the label. */
  title?: string;
}

export function Icon({ name, size = 16, className, title }: IconProps) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      {title ? <title>{title}</title> : null}
      {PATHS[name]}
    </svg>
  );
}
