/**
 * iperf 命令预览（提示性）
 *
 * 只读 mono 文本，实时随表单生成，一键复制。
 * 纯提示——不自动执行，复制后可到板子/命令行使用。
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyToClipboard } from "../../utils/clipboard";
import Icon from "../common/Icon";
import styles from "./IperfSessionView.module.css";

interface Props {
  command: string;
}

export default function IperfCommandPreview({ command }: Props) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>();

  // 卸载时清理"已复制"复位定时器
  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  const handleCopy = async () => {
    await copyToClipboard(command);
    setCopied(true);
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className={styles.commandPreview}>
      <div className={styles.commandHeader}>
        <span className={styles.commandLabel}>{t("iperf.commandPreview")}</span>
        <button
          className={`${styles.copyBtn} liquid-glass-ghost-button`}
          onClick={handleCopy}
          title={t("iperf.copyCommand")}
        >
          <Icon name="clipboard" size="sm" />
          {copied ? t("iperf.copied") : t("iperf.copyCommand")}
        </button>
      </div>
      <code className={styles.commandText}>{command}</code>
    </div>
  );
}
