import { Icon } from "../ui/Icon";
import { invoke } from "../platform/invoke";
import { useQuitConfirm } from "./QuitConfirm";

/**
 * The real OS window's own draggable title strip. The bracket mark +
 * wordmark and the Settings entry both moved into `Sidebar.tsx` once the
 * sidebar shell replaced the old bare-chrome layout, so this is down to
 * exactly what a borderless window still needs: hide and quit.
 * `data-tauri-drag-region` makes the whole bar draggable except the
 * buttons themselves.
 *
 * Minus hides Aside the same way ⌘⇧H does (from your screen, not from
 * screen sharing). Cross asks, then quits, the same way ⌘Q does.
 */
export function WindowChrome() {
  const quit = useQuitConfirm();

  return (
    <div className="aw-chrome" data-tauri-drag-region>
      <div className="aw-chrome__workspace" data-tauri-drag-region>
        <Icon name="mark" size={14} />
        <span>Aside</span>
        <span className="aw-chrome__workspace-label">Personal workspace</span>
      </div>
      <div className="aw-chrome__actions">
        <button
          type="button"
          className="aw-chrome__btn"
          aria-label="Hide"
          title="Hide (⌘⇧H)"
          onClick={() => void invoke("toggle_on_screen")}
        >
          <Icon name="minimize" />
        </button>
        <button
          type="button"
          className="aw-chrome__btn"
          aria-label="Quit"
          title="Quit (⌘Q)"
          onClick={quit.askToQuit}
        >
          <Icon name="close" />
        </button>
      </div>
      {quit.dialog}
    </div>
  );
}
