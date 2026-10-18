/** Workspace and its dialogs stay light; only the live window uses dark surfaces. */
export function applyTheme(live: boolean): void {
  document.documentElement.dataset.theme = live ? "dark" : "light";
  document.documentElement.classList.toggle("live-window", live);
}
