import { useStore } from "../core/store";
import { Icon, type IconName } from "../kit/Icon";
import { StatusDot } from "../kit/Bits";
import { systemStore } from "./data";
import { engineName, modelLabel } from "./labels";
import { aiProblem, speechProblem } from "./readiness";
import { hrefFor, useRoute, type PageId } from "./router";

const ITEMS: Array<{ page: PageId; label: string; icon: IconName }> = [
  { page: "copilots", label: "Copilots", icon: "spark" },
  { page: "resumes", label: "Resumes", icon: "doc" },
  { page: "documents", label: "Documents", icon: "note" },
  { page: "history", label: "History", icon: "clock" },
];

export function Sidebar() {
  const route = useRoute();
  const system = useStore(systemStore, (state) => state);
  const speech = speechProblem(system);
  const ai = aiProblem(system);

  const rows: Array<{ label: string; tone: "ok" | "warn" | "off"; section: string }> = !system.loaded
    ? [
        { label: "Speech…", tone: "off", section: "speech" },
        { label: "Model…", tone: "off", section: "ai" },
        { label: "Privacy…", tone: "off", section: "privacy" },
      ]
    : [
    { label: speech ? "Set up speech" : engineName(system.engine!), tone: speech ? "warn" : "ok", section: "speech" },
    { label: ai ? "Set up AI model" : modelLabel(system.provider!, system.model!).replace(/^[^·]+· /, ""), tone: ai ? "warn" : "ok", section: "ai" },
    { label: system.hidden === null ? "Sharing unknown" : system.hidden ? "Hidden from sharing" : "Visible to sharing", tone: system.hidden === null ? "off" : system.hidden ? "ok" : "warn", section: "privacy" },
  ];

  return (
    <nav className="hm-side" aria-label="Aside">
      <ul className="hm-nav">
        {ITEMS.map((item) => (
          <li key={item.page}>
            <a className="hm-nav__item" href={hrefFor(item.page)} aria-current={route.page === item.page ? "page" : undefined}>
              <Icon name={item.icon} size={17} />
              {item.label}
            </a>
          </li>
        ))}
        <li>
          <a className="hm-nav__item" href={hrefFor("settings")} aria-current={route.page === "settings" ? "page" : undefined}>
            <Icon name="gear" size={17} />
            Settings
          </a>
        </li>
      </ul>

      <div className="hm-systems">
        <p className="caption">Status</p>
        {rows.map((row) => (
          <a key={row.section} className="hm-systems__row" href={hrefFor("settings", row.section)}>
            <StatusDot tone={row.tone} />
            <span>{row.label}</span>
          </a>
        ))}
      </div>
    </nav>
  );
}
