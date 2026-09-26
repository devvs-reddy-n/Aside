import { useEffect, useRef, useState } from "react";

/**
 * Five bars that follow loudness: they jump up fast (50 ms) and fall back slowly (200 ms),
 * like a studio meter, so speech reads as steady rather than flickering. The animation
 * stops once the bars have settled, so a quiet meter costs nothing.
 */
export function LevelBars({ level, active, label }: { level: number; active: boolean; label?: string }) {
  const goal = active ? Math.min(1, Math.sqrt(level) * 3.4) : 0;
  const target = useRef(goal);
  const value = useRef(0);
  const frame = useRef(0);
  const [shown, setShown] = useState(0);
  target.current = goal;

  useEffect(() => {
    if (frame.current) return;
    let last = performance.now();
    const tick = (now: number) => {
      const dt = Math.min(64, now - last);
      last = now;
      const want = target.current;
      const tau = want > value.current ? 50 : 200;
      value.current += (want - value.current) * (1 - Math.exp(-dt / tau));
      if (Math.abs(want - value.current) < 0.005) value.current = want;
      setShown(value.current);
      frame.current = value.current === want ? 0 : requestAnimationFrame(tick);
    };
    frame.current = requestAnimationFrame(tick);
  }, [goal]);

  useEffect(() => () => {
    cancelAnimationFrame(frame.current);
    frame.current = 0;
  }, []);

  const lit = Math.round(shown * 5);
  return (
    <span className="bars" data-active={active || undefined} role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : true}>
      {[0, 1, 2, 3, 4].map((index) => <i key={index} data-on={index < lit || undefined} />)}
    </span>
  );
}
