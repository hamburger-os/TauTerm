import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../common/Icon";
import type { MatchMode, MatchFormat } from "./types";
import styles from "./AutoReplyRuleEditor.module.css";

interface MatchTesterProps {
  pattern: string;
  mode: MatchMode;
  caseSensitive: boolean;
  /** 匹配格式："text" 为标准文本匹配，"hex" 对十六进制字符串做字节级匹配 */
  matchFormat?: MatchFormat;
}

interface TestResult {
  valid: boolean;
  matched: boolean | null;
  groups: string[];
  error?: string;
}

export default function MatchTester({ pattern, mode, caseSensitive, matchFormat }: MatchTesterProps) {
  const { t } = useTranslation();
  const [testData, setTestData] = useState("");
  const [result, setResult] = useState<TestResult | null>(null);
  const [testing, setTesting] = useState(false);

  const isHex = matchFormat === "hex";

  const handleTest = useCallback(async () => {
    if (!pattern.trim()) return;
    setTesting(true);
    try {
      const res = await invoke<TestResult>("test_match", {
        pattern: pattern.trim(),
        mode,
        testData: testData || "",
        caseSensitive,
        matchFormat: isHex ? "hex" : "text",
      });
      setResult(res);
    } catch (e) {
      setResult({ valid: false, matched: null, groups: [], error: String(e) });
    } finally {
      setTesting(false);
    }
  }, [pattern, mode, testData, caseSensitive, isHex]);

  const showCaptureGroups = mode === "regex" && (result?.groups.length ?? 0) > 0;

  return (
    <div className={styles.regexTester}>
      <div className={styles.regexTesterHeader}>
        <span>{t("sendBar.matchTest")}</span>
      </div>
      <div className={styles.regexTesterBody}>
        <div className={styles.regexTesterInput}>
          <input
            className="liquid-glass-input"
            type="text"
            value={testData}
            onChange={e => setTestData(e.target.value)}
            placeholder={isHex ? t("sendBar.matchFormatHexPlaceholder") : t("sendBar.matchTestPlaceholder")}
          />
          <button
            className={`${styles.regexTestBtn} liquid-primary-button`}
            onClick={handleTest}
            disabled={!pattern.trim() || testing}
          >
            {t("sendBar.matchTestBtn")}
          </button>
        </div>
        {result && (
          <div className={`${styles.regexResult} ${result.valid && result.matched ? styles.regexSuccess : result.valid && result.matched === null ? styles.regexNoMatch : result.valid ? styles.regexNoMatch : styles.regexError}`}>
            {result.error
              ? <><Icon name="close" size="xs" /> {result.error}</>
              : result.matched === null
                ? <><Icon name="check-plain" size="xs" /> {t("sendBar.matchValid")}</>
                : result.matched
                  ? <><Icon name="check-plain" size="xs" /> {t("sendBar.matchSuccess")}</>
                  : <><Icon name="close" size="xs" /> {t("sendBar.matchFail")}</>
            }
            {showCaptureGroups && (
              <div className={styles.regexGroups}>
                <span className={styles.regexGroupsTitle}>
                  {t("sendBar.regexCaptureGroups")}:
                </span>
                {result.groups.map((g, i) => (
                  <code key={i} className={styles.regexGroupItem}>
                    [{i}] &quot;{g}&quot;
                  </code>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
