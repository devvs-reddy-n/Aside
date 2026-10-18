/** Use the same generated artwork in the sidebar, restore bubble and app icon. */
export function BrandMark({ size = 36 }: { size?: number }) {
  return <img className="brand-mark" src="/aside-icon.png" width={size} height={size} alt="" aria-hidden="true" draggable={false} />;
}
