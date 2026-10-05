/**
 * Aside has one look: dark and see-through, in both windows. The stamp stays
 * so styles keyed to `data-theme` keep working.
 */
export function applyTheme(): void {
  document.documentElement.dataset.theme = "dark";
}
