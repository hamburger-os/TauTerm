/**
 * 文件浏览器渲染器
 *
 * 双栏文件树布局（本地文件系统在左，远程文件系统在右）。
 * 适用于 FTP、NFS 等 `content_type: "file_browser"` 的插件。
 */

import type { FC } from "react";
import type { TabInfo } from "./types";
import styles from "./FileBrowserRenderer.module.css";

interface FileBrowserRendererProps {
  tab: TabInfo;
}

const FileBrowserRenderer: FC<FileBrowserRendererProps> = ({ tab }) => {
  return (
    <div className={styles.container}>
      <div className={styles.pane}>
        <div className={styles.paneHeader}>📁 本地文件</div>
        <div className={styles.paneContent}>
          <p className={styles.placeholder}>本地文件树（待实现）</p>
        </div>
      </div>
      <div className={styles.divider} />
      <div className={styles.pane}>
        <div className={styles.paneHeader}>🌐 远程文件 ({tab.name})</div>
        <div className={styles.paneContent}>
          <p className={styles.placeholder}>远程文件树（待实现）</p>
        </div>
      </div>
    </div>
  );
};

export default FileBrowserRenderer;
