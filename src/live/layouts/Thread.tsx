import type { Room } from "../hooks";
import { ActionBar } from "../parts/ActionBar";
import { Alerts } from "../parts/Alerts";
import { AudioLane, AudioRow } from "../parts/AudioRow";
import { Conversation } from "../parts/Conversation";
import { TopBar } from "../parts/TopBar";

/** Conversation thread: one scrolling list of what was said, with each answer right under its question. */
export function Thread({ room }: { room: Room }) {
  return (
    <>
      {room.height === "tiny" ? (
        <TopBar slim start={<><AudioLane channel="mic" iconOnly /><AudioLane channel="system" iconOnly /></>} />
      ) : (
        <>
          <TopBar />
          <AudioRow />
        </>
      )}
      <Alerts />
      <Conversation answers />
      <ActionBar mode={room.height === "regular" ? "full" : "slim"} />
    </>
  );
}
