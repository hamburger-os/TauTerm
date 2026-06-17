/**
 * 统计仪表盘渲染器
 *
 * 实时图表渲染面板，适用于 iPerf3、UDP Monitor 等
 * `content_type: "stats_dashboard"` 的插件。
 */

import type { FC } from "react";
import type { TabInfo } from "./types";

interface StatsDashboardRendererProps {
  tab: TabInfo;
}

const StatsDashboardRenderer: FC<StatsDashboardRendererProps> = ({ tab }) => {
  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span style={styles.title}>📊 {tab.name}</span>
        <span style={styles.badge}>统计仪表盘</span>
      </div>
      <div style={styles.grid}>
        <div style={styles.card}>
          <div style={styles.cardTitle}>吞吐量</div>
          <div style={styles.cardValue}>— Mbps</div>
          <div style={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div style={styles.card}>
          <div style={styles.cardTitle}>延迟 / 抖动</div>
          <div style={styles.cardValue}>— ms</div>
          <div style={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div style={styles.card}>
          <div style={styles.cardTitle}>丢包率</div>
          <div style={styles.cardValue}>— %</div>
          <div style={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div style={styles.card}>
          <div style={styles.cardTitle}>连接状态</div>
          <div style={styles.cardValue}>已连接</div>
        </div>
      </div>
    </div>
  );
};

const styles: Record<string, React.CSSProperties> = {
  container: {
    flex: 1,
    padding: 16,
    backgroundColor: "var(--bg-primary, #0a0a1a)",
    overflow: "auto",
  },
  header: {
    display: "flex",
    alignItems: "center",
    gap: 12,
    marginBottom: 16,
  },
  title: {
    fontSize: 16,
    fontWeight: 600,
    color: "var(--text-primary, #e0e0ff)",
  },
  badge: {
    fontSize: 11,
    padding: "2px 8px",
    borderRadius: 4,
    backgroundColor: "var(--accent-color, #00ffff)",
    color: "#000",
  },
  grid: {
    display: "grid",
    gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
    gap: 12,
  },
  card: {
    padding: 16,
    borderRadius: 8,
    backgroundColor: "var(--bg-secondary, #12122a)",
    border: "1px solid var(--border-color, rgba(0,255,255,0.15))",
  },
  cardTitle: {
    fontSize: 12,
    color: "var(--text-secondary, #888)",
    marginBottom: 8,
  },
  cardValue: {
    fontSize: 24,
    fontWeight: 700,
    color: "var(--text-primary, #e0e0ff)",
    marginBottom: 12,
  },
  chartPlaceholder: {
    height: 60,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontSize: 11,
    color: "var(--text-muted, #444)",
    border: "1px dashed var(--border-color, rgba(0,255,255,0.1))",
    borderRadius: 4,
  },
};

export default StatsDashboardRenderer;
