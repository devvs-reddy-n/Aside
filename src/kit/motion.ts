/**
 * Aside's motion vocabulary, shared by every animated piece (see tokens.css for the CSS side).
 * Nothing overshoots: the spring is critically damped. With Reduce Motion on, every timing
 * collapses to a 120 ms fade (Motion itself also drops movement then, via MotionConfig).
 */
const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;

export const SPRING = reduced ? ({ type: "tween", duration: 0.12 } as const) : ({ type: "spring", stiffness: 420, damping: 41 } as const);
export const EASE_OUT = [0.22, 1, 0.36, 1] as const;
export const FAST = 0.12;
export const BASE = reduced ? 0.12 : 0.18;
export const SLOW = reduced ? 0.12 : 0.26;
