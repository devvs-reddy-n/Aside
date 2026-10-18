import { useEffect } from "react";
import { ToastHost } from "../kit/toast";
import { LiveProvider, useLiveUi } from "./context";
import type { LiveController } from "./controller";
import { Alerts } from "./parts/Alerts";
import { Bar } from "./parts/Bar";
import { Composer, FollowUp, QuestionActions, useComposer } from "./parts/Composer";
import { Ended } from "./parts/Ended";
import { Reading } from "./parts/Reading";
import { ResizeHandles } from "./ResizeHandles";

/** Below this, a soft halo keeps letters readable over whatever shows through. */
const HALO_BELOW = 75;

/**
 * The live window's one panel: a top bar, the question and its answer, and a
 * reserved footer controls. Opacity sets how solid the panel is (20–100%);
 * Text size scales the question and answer. Text always stays solid.
 */
function Panel() {
  const composer = useComposer();
  const prefs = useLiveUi((state) => state.prefs);
  const style = { "--fs": prefs.font_px / 13, "--alpha": prefs.opacity_pct / 100 } as React.CSSProperties;
  return (
    <div className="lv-root" style={style}>
      <div className="lv-panel" data-thin={prefs.opacity_pct < HALO_BELOW || undefined}>
        <Bar />
        <Alerts />
        <Reading actions={<QuestionActions state={composer} />} followUp={<FollowUp state={composer} />} />
        <Composer state={composer} />
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
