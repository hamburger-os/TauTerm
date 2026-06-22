/**
 * 统计仪表盘渲染器
 *
 * 实时图表渲染面板，适用于 iPerf3、UDP Monitor 等
 * `content_type: "stats_dashboard"` 的插件。
 */

import type { FC } from "react";
import type { TabInfo } from "./types";
import styles from "./StatsDashboardRenderer.module.css";

interface StatsDashboardRendererProps {
  tab: TabInfo;
}

const StatsDashboardRenderer: FC<StatsDashboardRendererProps> = ({ tab }) => {
  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <span className={styles.title}>📊 {tab.name}</span>
        <span className={styles.badge}>统计仪表盘</span>
      </div>
      <div className={styles.grid}>
        <div className={styles.card}>
          <div className={styles.cardTitle}>吞吐量</div>
          <div className={styles.cardValue}>— Mbps</div>
          <div className={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div className={styles.card}>
          <div className={styles.cardTitle}>延迟 / 抖动</div>
          <div className={styles.cardValue}>— ms</div>
          <div className={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div className={styles.card}>
          <div className={styles.cardTitle}>丢包率</div>
          <div className={styles.cardValue}>— %</div>
          <div className={styles.chartPlaceholder}>[实时图表区域]</div>
        </div>
        <div className={styles.card}>
          <div className={styles.cardTitle}>连接状态</div>
          <div className={styles.cardValue}>已连接</div>
        </div>
      </div>
    </div>
  );
};

export default StatsDashboardRenderer;
