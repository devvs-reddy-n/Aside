/** "⌘ ⇧ ↩" → three little keys. */
export function Keys({ combo }: { combo: string }) {
  return (
    <span className="keys" aria-label={combo.replace(/\s+/g, " ")}>
      {combo.split(/\s+/).filter(Boolean).map((key, index) => <kbd key={index} className="keycap">{key}</kbd>)}
    </span>
  );
}
