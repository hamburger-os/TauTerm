/**
 * 带宽-时间曲线（零依赖 SVG 折线图）
 *
 * 全部颜色使用主题 CSS 变量（零硬编码），ResizeObserver 自适应宽度。
 * 数据点 = (time 秒, bandwidthMbps)。
 */
import { useEffect, useRef, useState } from "react";
import styles from "./IperfSessionView.module.css";

export interface ChartDataPoint {
  time: number;
  bandwidthMbps: number;
}

interface Props {
  dataPoints: ChartDataPoint[];
  height?: number;
}

/** Y 轴取整（向上取整到 1/2/5 序列） */
function niceMax(v: number): number {
  if (v <= 0) return 10;
  const exp = Math.floor(Math.log10(v));
  const base = Math.pow(10, exp);
  const frac = v / base;
  const nice = frac <= 1 ? 1 : frac <= 2 ? 2 : frac <= 5 ? 5 : 10;
  return nice * base;
}

export default function IperfBandwidthChart({ dataPoints, height = 160 }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const update = () => setWidth(el.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const padding = { top: 12, right: 16, bottom: 24, left: 56 };
  const plotW = Math.max(width - padding.left - padding.right, 10);
  const plotH = Math.max(height - padding.top - padding.bottom, 10);

  const yMax = niceMax(Math.max(...dataPoints.map((d) => d.bandwidthMbps), 1));
  const xMax = Math.max(...dataPoints.map((d) => d.time), 1);

  const xScale = (t: number) => padding.left + (t / xMax) * plotW;
  const yScale = (m: number) => padding.top + (1 - m / yMax) * plotH;

  const points = dataPoints
    .map((d) => `${xScale(d.time).toFixed(1)},${yScale(d.bandwidthMbps).toFixed(1)}`)
    .join(" ");

  // 网格刻度（Y: 0/25/50/75/100%）
  const yTicks = [0, 0.25, 0.5, 0.75, 1].map((f) => yMax * f);
  // X 刻度（最多 6 个）
  const xTickCount = Math.min(6, Math.max(2, Math.floor(xMax)));
  const xTicks = Array.from({ length: xTickCount + 1 }, (_, i) => (xMax * i) / xTickCount);

  return (
    <div ref={containerRef} className={styles.chartContainer}>
      {width > 0 && (
        <svg width={width} height={height} style={{ display: "block" }}>
          {/* 网格线 */}
          {yTicks.map((y, i) => (
            <g key={`y${i}`}>
              <line
                x1={padding.left}
                y1={yScale(y)}
                x2={padding.left + plotW}
                y2={yScale(y)}
                stroke="var(--glass-border-default)"
                strokeDasharray="4 4"
                strokeWidth={1}
              />
              <text
                x={padding.left - 6}
                y={yScale(y) + 3}
                fill="var(--text-muted)"
                fontSize={10}
                textAnchor="end"
              >
                {y >= 1000 ? `${(y / 1000).toFixed(1)}G` : y.toFixed(0)}
              </text>
            </g>
          ))}
          {/* X 轴刻度 */}
          {xTicks.map((x, i) => (
            <text
              key={`x${i}`}
              x={xScale(x)}
              y={height - 6}
              fill="var(--text-muted)"
              fontSize={10}
              textAnchor="middle"
            >
              {x.toFixed(0)}s
            </text>
          ))}
          {/* 坐标轴 */}
          <line
            x1={padding.left}
            y1={padding.top}
            x2={padding.left}
            y2={padding.top + plotH}
            stroke="var(--text-muted)"
            strokeWidth={1}
          />
          <line
            x1={padding.left}
            y1={padding.top + plotH}
            x2={padding.left + plotW}
            y2={padding.top + plotH}
            stroke="var(--text-muted)"
            strokeWidth={1}
          />
          {/* 带宽折线 */}
          {points && (
            <polyline
              points={points}
              fill="none"
              stroke="var(--color-info)"
              strokeWidth={2}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          )}
        </svg>
      )}
    </div>
  );
}
