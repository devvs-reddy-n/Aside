import { Card, Pill, PillBar } from "../../ui";
import { useTheme } from "../../state/theme";
import { SettingsBullets } from "./SettingsBullets";

/**
 * Light/dark toggle for the main window. A display preference, not app
 * data — saved on this device only, takes effect immediately. The live
 * overlay is unaffected: it always stays dark during a call regardless of
 * this setting.
 */
export function AppearanceCard() {
  const theme = useTheme();

  return (
    <Card className="layer-page__section" title="Appearance">
      <div className="step-body">
        <PillBar>
          <Pill active={theme.mode === "light"} onClick={() => theme.setMode("light")}>
            Light
          </Pill>
          <Pill active={theme.mode === "dark"} onClick={() => theme.setMode("dark")}>
            Dark
          </Pill>
        </PillBar>
        <SettingsBullets items={["Changes how Aside looks on this device. The live overlay always stays dark."]} />
      </div>
    </Card>
  );
}
