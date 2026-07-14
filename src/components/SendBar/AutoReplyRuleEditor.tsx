import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type {
  AutoReplyRule,
  MatchMode,
  MatchFormat,
  MatchCondition,
  ConditionLogic,
  TriggerType,
  ReplyAction,
} from "./types";
import MatchTester from "./MatchTester";
import ReplyActionEditor from "./ReplyActionEditor";
import { clampNumber } from "./numInput";
import styles from "./AutoReplyRuleEditor.module.css";

interface Props {
  rule: AutoReplyRule;
  onSave: (rule: AutoReplyRule) => void;
  onCancel: () => void;
}

type EditorTab = "match" | "reply" | "advanced";

/** HEX 字符串校验：忽略空白后需为偶数个十六进制字符 */
function isValidHex(s: string): boolean {
  const clean = s.replace(/\s/g, "");
  return /^[0-9a-fA-F]*$/.test(clean) && clean.length % 2 === 0;
}

/** 默认空条件 */
function emptyCondition(): MatchCondition {
  return { pattern: "", mode: "contains", caseSensitive: false, negate: false };
}

export default function AutoReplyRuleEditor({ rule, onSave, onCancel }: Props) {
  const { t } = useTranslation();

  const [activeTab, setActiveTab] = useState<EditorTab>("match");
  const [ruleLabel, setRuleLabel] = useState(rule.label || "");
  const [actions, setActions] = useState<ReplyAction[]>(rule.actions || []);
  const [cooldownMs, setCooldownMs] = useState(rule.cooldownMs);

  // 触发方式
  const [triggerType, setTriggerType] = useState<TriggerType>(rule.triggerType || "data");
  const [timerIntervalMs, setTimerIntervalMs] = useState(rule.timerIntervalMs ?? 1000);

  // 匹配条件（统一模型：至少 1 条）
  const [conditions, setConditions] = useState<MatchCondition[]>(
    rule.conditions && rule.conditions.length > 0 ? rule.conditions : [emptyCondition()]
  );
  const [conditionLogic, setConditionLogic] = useState<ConditionLogic>(rule.conditionLogic || "and");

  const isTimer = triggerType === "timer";
  const single = conditions.length === 1;

  // 匹配模式选项
  const allMatchModes: { value: MatchMode; label: string }[] = [
    { value: "contains", label: t("sendBar.matchContains") },
    { value: "equals", label: t("sendBar.matchEquals") },
    { value: "starts_with", label: t("sendBar.matchStartsWith") },
    { value: "regex", label: t("sendBar.matchRegex") },
    { value: "lua_pattern", label: t("sendBar.matchLuaPattern") },
  ];
  const hexMatchModes = allMatchModes.filter(
    m => m.value === "contains" || m.value === "equals" || m.value === "starts_with"
  );
  const modesFor = (fmt: MatchFormat | undefined) => (fmt === "hex" ? hexMatchModes : allMatchModes);

  // ── 条件编辑 ──
  const updateCondition = (idx: number, patch: Partial<MatchCondition>) => {
    setConditions(conditions.map((c, i) => (i === idx ? { ...c, ...patch } : c)));
  };
  const setConditionFormat = (idx: number, fmt: MatchFormat) => {
    const c = conditions[idx];
    // 切换到 hex 时，若当前模式不支持则重置为 contains
    const modeOk = c.mode === "contains" || c.mode === "equals" || c.mode === "starts_with";
    updateCondition(idx, {
      matchFormat: fmt === "hex" ? "hex" : undefined,
      mode: fmt === "hex" && !modeOk ? "contains" : c.mode,
    });
  };
  const addCondition = () => setConditions([...conditions, emptyCondition()]);
  const removeCondition = (idx: number) => {
    if (conditions.length <= 1) return;
    setConditions(conditions.filter((_, i) => i !== idx));
  };

  // 校验：定时器需动作；data 需至少一条有 pattern，且 hex 条件需合法
  const hexInvalid = conditions.some(
    c => c.matchFormat === "hex" && c.pattern.trim().length > 0 && !isValidHex(c.pattern)
  );
  const canSave = (() => {
    if (isTimer) return actions.length > 0;
    if (hexInvalid) return false;
    return conditions.some(c => c.pattern.trim().length > 0);
  })();

  const handleSave = () => {
    if (!canSave) return;
    const base = {
      ...rule,
      label: ruleLabel.trim() || undefined,
      actions,
      cooldownMs,
    };

    if (isTimer) {
      onSave({
        ...base,
        triggerType: "timer",
        timerIntervalMs: Math.max(1, timerIntervalMs),
        conditions: [],
        conditionLogic,
      });
      return;
    }

    onSave({
      ...base,
      triggerType: "data",
      timerIntervalMs,
      conditions: conditions
        .filter(c => c.pattern.trim().length > 0)
        .map(c => ({ ...c, pattern: c.pattern.trim() })),
      conditionLogic,
    });
  };

  return createPortal(
    <div className={`${styles.overlay} glass-overlay`} onClick={onCancel}>
      <div className={`${styles.modal} liquid-glass`} onClick={e => e.stopPropagation()}>
        <h3 className={styles.title}>{t("sendBar.editRule")}</h3>

        {/* Tab 导航 */}
        <div className={styles.tabs}>
          <button
            className={`${styles.tab} ${activeTab === "match" ? styles.tabActive : ""}`}
            onClick={() => setActiveTab("match")}
          >
            {t("sendBar.tabMatch")}
          </button>
          <button
            className={`${styles.tab} ${activeTab === "reply" ? styles.tabActive : ""}`}
            onClick={() => setActiveTab("reply")}
          >
            {t("sendBar.tabReply")}
          </button>
          <button
            className={`${styles.tab} ${activeTab === "advanced" ? styles.tabActive : ""}`}
            onClick={() => setActiveTab("advanced")}
          >
            {t("sendBar.tabAdvanced")}
          </button>
        </div>

        <div className={styles.body}>
          {/* ── Tab 1: 匹配触发 ── */}
          {activeTab === "match" && (
            <div className={styles.tabContent}>
              <div className={styles.field}>
                <label>{t("sendBar.ruleLabel")}</label>
                <input
                  className="liquid-glass-input"
                  type="text"
                  value={ruleLabel}
                  onChange={e => setRuleLabel(e.target.value)}
                  placeholder={t("sendBar.ruleLabelPlaceholder")}
                />
              </div>

              {/* 触发方式 */}
              <div className={styles.field}>
                <label>{t("sendBar.triggerType")}</label>
                <select
                  className="liquid-glass-input liquid-glass-select"
                  value={triggerType}
                  onChange={e => setTriggerType(e.target.value as TriggerType)}
                >
                  <option value="data">{t("sendBar.triggerTypeData")}</option>
                  <option value="timer">{t("sendBar.triggerTypeTimer")}</option>
                </select>
              </div>

              {isTimer ? (
                /* 定时触发：仅间隔 */
                <div className={styles.field}>
                  <label>{t("sendBar.timerInterval")}</label>
                  <input
                    className="liquid-glass-input"
                    type="number"
                    value={timerIntervalMs}
                    onChange={e => setTimerIntervalMs(clampNumber(e.target.value, 1))}
                    min={1}
                    step={100}
                  />
                  <span className={styles.hint}>
                    {t("sendBar.timerIntervalHint")}
                  </span>
                </div>
              ) : (
                <>
                  {/* 条件逻辑（多条件时显示） */}
                  {!single && (
                    <div className={styles.field}>
                      <label>{t("sendBar.conditionLogic")}</label>
                      <select
                        className="liquid-glass-input liquid-glass-select"
                        value={conditionLogic}
                        onChange={e => setConditionLogic(e.target.value as ConditionLogic)}
                      >
                        <option value="and">{t("sendBar.conditionAnd")}</option>
                        <option value="or">{t("sendBar.conditionOr")}</option>
                      </select>
                    </div>
                  )}

                  {/* 条件列表 */}
                  <div className={styles.field}>
                    <label>{t("sendBar.conditions")}</label>
                    {conditions.map((c, idx) => {
                      const cHexInvalid = c.matchFormat === "hex" && c.pattern.trim().length > 0 && !isValidHex(c.pattern);
                      return (
                        <div key={idx} className={styles.conditionBlock}>
                          <div className={styles.conditionRow}>
                            <select
                              className="liquid-glass-input liquid-glass-select"
                              value={c.matchFormat === "hex" ? "hex" : "text"}
                              onChange={e => setConditionFormat(idx, e.target.value as MatchFormat)}
                              title={t("sendBar.matchFormat")}
                            >
                              <option value="text">{t("sendBar.matchFormatText")}</option>
                              <option value="hex">{t("sendBar.matchFormatHex")}</option>
                            </select>
                            <select
                              className="liquid-glass-input liquid-glass-select"
                              value={c.mode}
                              onChange={e => updateCondition(idx, { mode: e.target.value as MatchMode })}
                              title={t("sendBar.matchMode")}
                            >
                              {modesFor(c.matchFormat).map(m => (
                                <option key={m.value} value={m.value}>{m.label}</option>
                              ))}
                            </select>
                            <input
                              className="liquid-glass-input"
                              type="text"
                              value={c.pattern}
                              onChange={e => updateCondition(idx, { pattern: e.target.value })}
                              placeholder={
                                c.matchFormat === "hex"
                                  ? (t("sendBar.matchFormatHexPlaceholder"))
                                  : (t("sendBar.matchPattern"))
                              }
                            />
                            {c.matchFormat !== "hex" && (
                              <label className={styles.conditionCheck} title={t("sendBar.caseSensitive")}>
                                <input
                                  type="checkbox"
                                  className={styles.checkInput}
                                  checked={c.caseSensitive}
                                  onChange={e => updateCondition(idx, { caseSensitive: e.target.checked })}
                                />
                                <div className={styles.checkTrack} />
                                Aa
                              </label>
                            )}
                            <label className={styles.conditionCheck} title={t("sendBar.conditionNegate")}>
                              <input
                                type="checkbox"
                                className={styles.checkInput}
                                checked={c.negate}
                                onChange={e => updateCondition(idx, { negate: e.target.checked })}
                              />
                              <div className={styles.checkTrack} />
                              !
                            </label>
                            <button
                              type="button"
                              className={`${styles.conditionRemove} liquid-glass-button`}
                              onClick={() => removeCondition(idx)}
                              disabled={single}
                              title={t("sendBar.removeCondition")}
                            >
                              ✕
                            </button>
                          </div>
                          {cHexInvalid && (
                            <span className={styles.hintError}>{t("sendBar.invalidHex")}</span>
                          )}
                          {c.matchFormat !== "hex" && (c.mode === "contains" || c.mode === "equals" || c.mode === "starts_with") && c.pattern.includes('\\') && (
                            <span className={styles.hint}>
                              {t("sendBar.escapeHint")}
                            </span>
                          )}
                        </div>
                      );
                    })}
                    <button
                      type="button"
                      className={`${styles.actionAdd} liquid-glass-button`}
                      onClick={addCondition}
                    >
                      + {t("sendBar.addCondition")}
                    </button>
                  </div>

                  {/* 匹配测试器：对首个有 pattern 的条件进行测试 */}
                  {(() => {
                    const firstCond = conditions.find(c => c.pattern.trim().length > 0);
                    if (!firstCond) return (
                      <div className={styles.matchTesterHint}>{t("sendBar.matchTesterHint")}</div>
                    );
                    return (
                      <MatchTester
                        pattern={firstCond.pattern}
                        mode={firstCond.mode}
                        caseSensitive={firstCond.caseSensitive}
                        matchFormat={firstCond.matchFormat}
                      />
                    );
                  })()}
                </>
              )}
            </div>
          )}

          {/* ── Tab 2: 回复动作 ── */}
          {activeTab === "reply" && (
            <div className={styles.tabContent}>
              <div className={styles.field}>
                <label>{t("sendBar.actions")}</label>
                <ReplyActionEditor
                  actions={actions}
                  onChange={setActions}
                />
              </div>
            </div>
          )}

          {/* ── Tab 3: 高级选项 ── */}
          {activeTab === "advanced" && (
            <div className={styles.tabContent}>
              <div className={styles.field}>
                <label>{t("sendBar.cooldownMs")}</label>
                <input
                  className="liquid-glass-input"
                  type="number"
                  value={cooldownMs}
                  onChange={e => setCooldownMs(clampNumber(e.target.value, 0, 60000))}
                  min={0}
                  max={60000}
                  step={100}
                  placeholder="0 = unlimited"
                />
                <span className={styles.hint}>
                  {t("sendBar.cooldownHint")}
                </span>
              </div>
            </div>
          )}
        </div>

        {/* 底部按钮 */}
        <div className={styles.buttons}>
          <button className={`${styles.cancelBtn} liquid-glass-button`} onClick={onCancel}>
            {t("sendBar.cancel")}
          </button>
          <button className={`${styles.saveBtn} liquid-primary-button`} onClick={handleSave} disabled={!canSave}>
            {t("sendBar.save")}
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}
