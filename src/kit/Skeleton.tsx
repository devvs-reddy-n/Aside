/** Placeholder lines shaped like the text that is loading. */
export function Skeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="skeleton" aria-hidden="true">
      {Array.from({ length: lines }, (_, index) => <i key={index} style={{ width: index === lines - 1 ? "62%" : "100%" }} />)}
    </div>
  );
}
