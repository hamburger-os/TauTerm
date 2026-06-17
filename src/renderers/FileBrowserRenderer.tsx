/**
 * 文件浏览器渲染器
 *
 * 双栏文件树布局（本地文件系统在左，远程文件系统在右）。
 * 适用于 FTP、NFS 等 `content_type: "file_browser"` 的插件。
 */

import type { FC } from "react";
import type { TabInfo } from "./types";

interface FileBrowserRendererProps {
  tab: TabInfo;
}

const FileBrowserRenderer: FC<FileBrowserRendererProps> = ({ tab }) => {
  return (
    <div style={styles.container}>
      <div style={styles.pane}>
        <div style={styles.paneHeader}>📁 本地文件</div>
        <div style={styles.paneContent}>
          <p style={styles.placeholder}>本地文件树（待实现）</p>
        </div>
      </div>
      <div style={styles.divider} />
      <div style={styles.pane}>
        <div style={styles.paneHeader}>🌐 远程文件 ({tab.name})</div>
        <div style={styles.paneContent}>
          <p style={styles.placeholder}>远程文件树（待实现）</p>
        </div>
      </div>
    </div>
  );
};

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    flex: 1,
    height: "100%",
    backgroundColor: "var(--bg-primary, #0a0a1a)",
  },
  pane: {
    flex: 1,
    display: "flex",
    flexDirection: "column",
    overflow: "hidden",
  },
  paneHeader: {
    padding: "8px 12px",
    fontSize: 12,
    fontWeight: 600,
    color: "var(--text-secondary, #888)",
    borderBottom: "1px solid var(--border-color, rgba(0,255,255,0.15))",
  },
  paneContent: {
    flex: 1,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    overflow: "auto",
  },
  divider: {
    width: 1,
    backgroundColor: "var(--border-color, rgba(0,255,255,0.15))",
  },
  placeholder: {
    color: "var(--text-muted, #555)",
    fontSize: 14,
  },
};

export default FileBrowserRenderer;
