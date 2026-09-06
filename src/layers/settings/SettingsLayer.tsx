import { useSttEngine } from "../../state/sttEngine";
import { SpeechEngineCard } from "./SpeechEngineCard";
import { ProviderModelCard } from "./ProviderModelCard";
import { ServiceTierCard } from "./ServiceTierCard";
import { AnswerRulesCard } from "./AnswerRulesCard";
import { StealthCard } from "./StealthCard";
import { AppearanceCard } from "./AppearanceCard";

/**
 * Settings: speech and AI model on one row, then speed vs. cost, screen
 * sharing, appearance, and the strict answer rules. Audio device pickers
 * live in the live overlay toolbar.
 */
export function SettingsLayer() {
  const stt = useSttEngine();

  return (
    <div className="layer-page">
      <div className="layer-page__head">
        <h1 className="layer-page__title">Settings</h1>
      </div>

      <div className="settings-core-grid">
        <SpeechEngineCard stt={stt} />
        <ProviderModelCard />
      </div>
      <ServiceTierCard />
      <StealthCard />
      <AppearanceCard />
      <AnswerRulesCard />
    </div>
  );
}
