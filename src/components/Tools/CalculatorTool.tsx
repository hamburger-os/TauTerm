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
  const [activeTab, setActiveTab] = useState<CalcTab>("dataInspector");

  return (
    <RightSidebarPanel title={t("tools.calculator")}>
      <div className={styles.container}>
        <div className={styles.tabScroller}>
          <div className={styles.tabRow + " liquid-selector-strip"}>
            {TABS.map((tab) => (
              <button
                key={tab}
                className={styles.tabBtn + " liquid-glass-button liquid-selector-button " + (activeTab === tab ? "active liquid-theme-selected" : "")}
                onClick={() => setActiveTab(tab)}
                type="button"
                aria-pressed={activeTab === tab}
              >
                {t("tools.toolTabs." + tab)}
              </button>
            ))}
          </div>
        </div>

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
