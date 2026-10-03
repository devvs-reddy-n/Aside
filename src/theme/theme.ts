/**
 * Aside has one look, not light and dark modes: the workspace is always light,
 * and the live window draws its own fixed, clear style. The stamp stays so
 * styles keyed to `data-theme="light"` keep working.
 */
export function applyTheme(): void {
  document.documentElement.dataset.theme = "light";
}
