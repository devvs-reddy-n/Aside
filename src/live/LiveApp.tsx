import { useEffect } from "react";
import { ToastHost } from "../kit/toast";
import { LiveProvider, useLiveUi } from "./context";
import type { LiveController } from "./controller";
import { useRoom } from "./hooks";
import { Focus } from "./layouts/Focus";
import { Prompter } from "./layouts/Prompter";
import { Thread } from "./layouts/Thread";
import { Ended } from "./parts/Ended";
import { ResizeHandles } from "./ResizeHandles";

/** Below this, a soft halo keeps letters readable over whatever shows through. */
const HALO_BELOW = 75;

/**
 * The live window's one panel. The Background slider sets how solid the panel,
 * cards, buttons and code block are (40–100%); text always stays solid.
 */
function Panel() {
  const room = useRoom();
  const prefs = useLiveUi((state) => state.prefs);
  const layout = useLiveUi((state) => state.layout);
  const style = { "--fs": prefs.font_px / 13, "--alpha": prefs.opacity_pct / 100 } as React.CSSProperties;
  return (
    <div className="lv-root" data-w={room.width} data-h={room.height} style={style}>
      <div className="lv-panel" data-layout={layout} data-thin={prefs.opacity_pct < HALO_BELOW || undefined}>
        {layout === "thread" ? <Thread room={room} /> : layout === "prompter" ? <Prompter /> : <Focus room={room} />}
        <Ended />
      </div>
      <ResizeHandles />
    </div>
  );
}

/** Root of the live window (the second, always-on-top Aside window). */
export function LiveApp({ controller }: { controller: LiveController }) {
  useEffect(() => {
    document.documentElement.classList.add("live-window");
    return controller.start();
  }, [controller]);

  return (
    <LiveProvider controller={controller}>
      <Panel />
      <ToastHost />
    </LiveProvider>
  );
}
