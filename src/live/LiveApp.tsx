import { useEffect } from "react";
import { windows } from "../bridge/api";
import { formatClock } from "../core/format";
import { Icon } from "../kit/Icon";
import { ToastHost } from "../kit/toast";
import { LiveProvider, useLive, useLiveUi } from "./context";
import type { LiveController } from "./controller";
import { Feed } from "./Feed";
import { Header } from "./Header";
import { useRoom } from "./hooks";
import { Notices } from "./Notices";
import { Strip } from "./Strip";
import { Footer } from "./Tray";

function EndedPanel() {
  const ended = useLive((state) => state.ended);
  if (!ended) return null;
  return (
    <div className="lv-ended" role="status">
      <span className="lv-ended__check"><Icon name="check" size={26} /></span>
      <h2>Session ended</h2>
      <p>Transcript, notes and answers are saved to History.</p>
      <dl>
        <div><dt>Length</dt><dd>{formatClock(ended.elapsedMs)}</dd></div>
        <div><dt>Turns</dt><dd>{ended.turns}</dd></div>
        <div><dt>Answers</dt><dd>{ended.answers}</dd></div>
      </dl>
    </div>
  );
}

function Dock() {
  const room = useRoom();
  const tiny = room.height === "tiny";
  return (
    <div className="lv-dock" data-w={room.width} data-h={room.height}>
      <Header room={room} />
      {tiny ? null : <Strip room={room} />}
      <Notices />
      <Feed />
      <Footer room={room} />
      <EndedPanel />
    </div>
  );
}

function Frame() {
  const room = useRoom();
  const prefs = useLiveUi((state) => state.prefs);
  const style = { "--fs": prefs.font_px / 13, "--alpha": prefs.opacity_pct / 100 } as React.CSSProperties;
  return (
    <div className="lv-root" data-w={room.width} style={style}>
      <Dock />
    </div>
  );
}

/** Root of the live window (the second, always-on-top Aside window). */
export function LiveApp({ controller }: { controller: LiveController }) {
  useEffect(() => {
    document.documentElement.classList.add("live-window");
    return controller.start();
  }, [controller]);

  // ⌘Q asks to quit from here too; the confirmation itself appears in the main window.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "q") {
        event.preventDefault();
        void windows.requestQuit();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <LiveProvider controller={controller}>
      <Frame />
      <ToastHost />
    </LiveProvider>
  );
}
