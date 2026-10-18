import { desktopPlatform, screenCaptureHelp } from "../core/platform.ts";
/** Turns a raw backend failure into plain words plus, where useful, what to do about it. */
export interface FriendlyError {
  title: string;
  hint: string | null;
  /** Set when the fix is in Aside's own Settings. */
  settings: boolean;
  screenPermission?: boolean;
}

export function describeAnswerError(raw: string): FriendlyError {
  const message = raw.toLowerCase();
  if (/screen.*(permission|recording|access)|privacy.*screen|can.t see your screen/.test(message)) {
    return { title: "Screen access is needed", hint: screenCaptureHelp(), settings: false, screenPermission: desktopPlatform !== "windows" };
  }
  if (/api key|unauthori[sz]ed|401|403|invalid.*key|incorrect.*key|no key/.test(message)) {
    return { title: "The AI key isn't working", hint: "Check the key in Aside → Settings → AI model.", settings: true };
  }
  if (/rate.?limit|429|quota|billing|insufficient/.test(message)) {
    return { title: "The AI service is busy or out of credit", hint: "Wait a moment and try again, or check your plan with the provider.", settings: false };
  }
  if (/network|offline|internet|dns|resolve|connect|timed? ?out|unreachable|50[234]/.test(message)) {
    return { title: "Can't reach the AI service", hint: "Check your internet connection. Listening keeps running.", settings: false };
  }
  if (/wait a moment|already/.test(message)) {
    return { title: "One answer at a time", hint: "Wait for the current answer to finish.", settings: false };
  }
  return { title: "That answer didn't come through", hint: raw.length < 160 ? raw : null, settings: false };
}

/** Whether a channel failure reads as a macOS permission problem. */
export function looksLikePermissionProblem(message: string | null): boolean {
  if (!message) return true;
  return /permission|privacy|denied|not permitted|not allowed|no audio|screen|record/i.test(message);
}
