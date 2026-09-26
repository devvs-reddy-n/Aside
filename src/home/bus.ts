/**
 * Tiny one-way messages between parts of the workspace window, for actions
 * started far from where they happen (the ⌘P palette asking the copilot page
 * to open its Start checks, for example).
 */
export type BusMessage = "start-session" | "add-files" | "open-palette";

export function send(message: BusMessage): void {
  window.dispatchEvent(new CustomEvent(`aside:${message}`));
}

export function onMessage(message: BusMessage, handler: () => void): () => void {
  const listener = () => handler();
  window.addEventListener(`aside:${message}`, listener);
  return () => window.removeEventListener(`aside:${message}`, listener);
}
