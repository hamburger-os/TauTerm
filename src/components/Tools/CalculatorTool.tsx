import { useState } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import { ChecksumToolInner } from "./ChecksumTool";
import { EncodingToolInner } from "./EncodingTool";
import { BitOpsToolInner } from "./BitOpsTool";
import styles from "./CalculatorTool.module.css";

type CalcTab = "checksum" | "encoding" | "bitops";

const TABS: CalcTab[] = ["checksum", "encoding", "bitops"];

/**
 * 计算器面板 — 将校验和计算、编码转换、位操作合并为标签页切换
 */
export default function CalculatorTool() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<CalcTab>("checksum");

  return (
    <RightSidebarPanel title={t("tools.calculator") ?? "Quick Tools"}>
      <div className={styles.container}>
        {/* 标签栏 */}
        <div className={styles.tabRow}>
          {TABS.map((tab) => (
            <button
              key={tab}
              className={`${styles.tabBtn} ${activeTab === tab ? styles.active : ""}`}
              onClick={() => setActiveTab(tab)}
            >
              {t(`tools.${tab}`)}
            </button>
          ))}
        </div>

        {/* 标签内容 */}
        <div className={styles.tabBody}>
          {activeTab === "checksum" && <ChecksumToolInner />}
          {activeTab === "encoding" && <EncodingToolInner />}
          {activeTab === "bitops" && <BitOpsToolInner />}
        </div>
      </div>
    </RightSidebarPanel>
  );
}
