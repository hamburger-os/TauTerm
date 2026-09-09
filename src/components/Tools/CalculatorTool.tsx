import { useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import { ChecksumToolInner } from "./ChecksumTool";
import { EncodingToolInner } from "./EncodingTool";
import { BitOpsToolInner } from "./BitOpsTool";
import DataInspectorTool from "./DataInspectorTool";
import EngineeringTool from "./EngineeringTool";
import styles from "./CalculatorTool.module.css";

type CalcTab =
  | "checksum"
  | "encoding"
  | "bitops"
  | "dataInspector"
  | "engineering";

const TABS: CalcTab[] = [
  "checksum",
  "encoding",
  "bitops",
  "dataInspector",
  "engineering",
];

export default function CalculatorTool() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<CalcTab>(() => TABS[0]);

  return (
    <RightSidebarPanel title={t("tools.calculator")}>
      <div className={styles.container}>
        <select
          className={styles.select + " liquid-glass-input liquid-glass-select"}
          value={activeTab}
          onChange={(event) => setActiveTab(event.target.value as CalcTab)}
          aria-label={t("tools.calculator")}
        >
          {TABS.map((tab) => (
            <option key={tab} value={tab}>
              {t("tools.toolTabs." + tab)}
            </option>
          ))}
        </select>

        <div className={styles.tabBody}>
          <div className={activeTab === "checksum" ? styles.tabPanel : styles.tabPanelHidden}>
            <ChecksumToolInner />
          </div>
          <div className={activeTab === "encoding" ? styles.tabPanel : styles.tabPanelHidden}>
            <EncodingToolInner />
          </div>
          <div className={activeTab === "bitops" ? styles.tabPanel : styles.tabPanelHidden}>
            <BitOpsToolInner />
          </div>
          <div className={activeTab === "dataInspector" ? styles.tabPanel : styles.tabPanelHidden}>
            <DataInspectorTool />
          </div>
          <div className={activeTab === "engineering" ? styles.tabPanel : styles.tabPanelHidden}>
            <EngineeringTool />
          </div>
        </div>
      </div>
    </RightSidebarPanel>
  );
}
