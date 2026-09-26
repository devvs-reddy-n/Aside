/** Aside's motion vocabulary, shared by every animated piece (see tokens.css for the CSS side). */
export const SPRING = { type: "spring", stiffness: 420, damping: 34 } as const;
export const EASE_OUT = [0.22, 1, 0.36, 1] as const;
export const FAST = 0.12;
export const BASE = 0.18;
export const SLOW = 0.26;
