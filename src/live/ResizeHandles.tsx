import type { ResizeDirection } from "./backend";
import { useController } from "./context";

const DIRECTIONS: ResizeDirection[] = ["North", "NorthEast", "East", "SouthEast", "South", "SouthWest", "West", "NorthWest"];

/** Frameless windows need explicit drag areas along their transparent border. */
export function ResizeHandles() {
  const { backend } = useController();
  return DIRECTIONS.map((direction) => (
    <div
      key={direction}
      className={`lv-resize lv-resize--${direction}`}
      aria-hidden="true"
      title="Drag to resize window"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.stopPropagation();
        backend.startResize(direction);
      }}
    />
  ));
}
