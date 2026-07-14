import { useState, useCallback, useEffect, useMemo, useRef, Fragment } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { useSendBar } from "./SendBarContext";
import { usePointerDragReorder } from "../../hooks/usePointerDragReorder";
import Icon from "../common/Icon";
import AutoReplyRuleEditor from "./AutoReplyRuleEditor";
import type { AutoReplyRule, AutoReplyConfig, MatchStrategy, ScriptRecord } from "./types";
import { BUILTIN_CONFIGS } from "./builtinRules";
import styles from "./AutoReplyPanel.module.css";

interface AutoReplyPanelProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

const STORAGE_KEY_CONFIGS = "tauterm-auto-reply-configs";
const STORAGE_KEY_ACTIVE = "tauterm-active-auto-reply-config";

// 匹配模式 → i18n key（与 AutoReplyRuleEditor 的模式选项一致）
const MATCH_MODE_KEY: Record<string, string> = {
  contains: "matchContains",
  equals: "matchEquals",
  starts_with: "matchStartsWith",
  regex: "matchRegex",
  lua_pattern: "matchLuaPattern",
};

function makeId(): string {
  return crypto.randomUUID();
}

function defaultConfig(): AutoReplyConfig {
  return { name: "New Config", matchStrategy: "all", rules: [] };
}

export default function AutoReplyPanel({ sessionId, isActive, onRunningChange }: AutoReplyPanelProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const { showToast } = useToast();
  const activeTab = sessionState.tabs.find(tab => tab.id === sessionId);
  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";

  const { state: sendBarState, dispatch } = useSendBar();
  const { configs, activeConfigName, rules, isRunning, matchStrategy } = sendBarState.autoReply;
  const { scripts } = sendBarState.script;
  const scriptLogs = sendBarState.scriptLogs;

  const [editorOpen, setEditorOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<AutoReplyRule | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [configDeleteConfirm, setConfigDeleteConfirm] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [importData, setImportData] = useState<AutoReplyConfig | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [logExpanded, setLogExpanded] = useState(false);

  const listRef = useRef<HTMLDivElement>(null);

  // 激活当前配置的规则
  const activeConfig = useMemo(
    () => configs.find(c => c.name === activeConfigName) ?? configs[0],
    [configs, activeConfigName]
  );

  useEffect(() => {
    if (activeConfig) {
      dispatch({ type: "SET_AUTO_REPLY_RULES", rules: activeConfig.rules });
      // 同步匹配策略，避免切换配置后策略与配置不一致
      dispatch({ type: "SET_MATCH_STRATEGY", strategy: activeConfig.matchStrategy });
    }
  }, [activeConfigName, activeConfig, dispatch]);

  // 切换配置时重置删除确认（配置集与规则行）
  useEffect(() => {
    setConfigDeleteConfirm(false);
    setDeleteConfirmId(null);
  }, [activeConfigName]);

  // 会话断开时自动停止自动应答
  useEffect(() => {
    const unlisten = listen<{ session_id: string }>("session-disconnected", (event) => {
      if (event.payload.session_id === sessionId && isRunning) {
        dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: false });
        onRunningChange?.(false);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, isRunning, dispatch, onRunningChange]);

  // 错误级日志以 Toast 呈现（共享日志已由 SendBarInner 层始终监听）
  useEffect(() => {
    const lastMsg = scriptLogs[scriptLogs.length - 1];
    if (lastMsg && (lastMsg.includes("失败") || lastMsg.includes("错误") || lastMsg.includes("Error"))) {
      // 仅当当前面板活跃且引擎运行时弹 Toast，避免重复打扰
      if (isActive && isRunning) {
        showToast("error", lastMsg);
      }
    }
  }, [scriptLogs, isActive, isRunning, showToast]);

  // 持久化
  const persist = useCallback((updated: AutoReplyConfig[]) => {
    dispatch({ type: "SET_AUTO_REPLY_CONFIGS", configs: updated });
    localStorage.setItem(STORAGE_KEY_CONFIGS, JSON.stringify(updated));
  }, [dispatch]);

  const persistActive = useCallback((name: string) => {
    dispatch({ type: "SET_ACTIVE_AUTO_REPLY_CONFIG", name });
    localStorage.setItem(STORAGE_KEY_ACTIVE, name);
  }, [dispatch]);

  // 更新规则并写回当前配置（context + localStorage）
  const persistRules = useCallback((updated: AutoReplyRule[]) => {
    dispatch({ type: "SET_AUTO_REPLY_RULES", rules: updated });
    const updatedConfigs = configs.map(c =>
      c.name === activeConfigName ? { ...c, rules: updated } : c
    );
    persist(updatedConfigs);
  }, [configs, activeConfigName, dispatch, persist]);

  // 拖拽排序（使用通用 hook，与 ReplyActionEditor 保持一致；必须在 persistRules 之后声明）
  const {
    isDragging,
    dropIndex,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
  } = usePointerDragReorder(rules, persistRules, {
    itemSelector: `.${styles.ruleRow}`,
    draggingClass: styles.rowDragging,
    disabled: isRunning,
    listRef,
  });

  // ── 配置管理 ──
  const handleSelectConfig = useCallback((name: string) => {
    persistActive(name);
  }, [persistActive]);

  const handleNewConfig = useCallback(() => {
    const name = t("sendBar.newConfigName", { n: configs.length + 1 });
    const updated = [...configs, { ...defaultConfig(), name }];
    persist(updated);
    persistActive(name);
  }, [configs, persist, persistActive]);

  const handleRenameConfig = useCallback(() => {
    const currentName = activeConfig?.name || activeConfigName || "";
    setRenameValue(currentName);
    setRenameOpen(true);
  }, [activeConfig, activeConfigName]);

  const handleConfirmRename = useCallback(() => {
    const newName = renameValue.trim();
    if (!newName || newName === activeConfigName) {
      setRenameOpen(false);
      return;
    }
    const updated = configs.map(c =>
      c.name === activeConfigName ? { ...c, name: newName } : c
    );
    persist(updated);
    persistActive(newName);
    setRenameOpen(false);
  }, [renameValue, activeConfigName, configs, persist, persistActive]);

  const handleCancelRename = useCallback(() => {
    setRenameOpen(false);
  }, []);

  const handleDeleteConfig = useCallback(() => {
    if (configs.length === 0) return;
    if (!configDeleteConfirm) {
      setConfigDeleteConfirm(true);
      return;
    }
    // 确认删除
    const updated = configs.filter(c => c.name !== activeConfigName);
    persist(updated);
    persistActive(updated[0]?.name || "");
    // 删除最后一个配置：清空规则，避免底部状态/开始按钮引用旧规则
    if (updated.length === 0) {
      dispatch({ type: "SET_AUTO_REPLY_RULES", rules: [] });
    }
    setConfigDeleteConfirm(false);
  }, [activeConfigName, configs, configDeleteConfirm, persist, persistActive, dispatch]);

  // ── 规则管理 ──
  const handleAddRule = useCallback(() => {
    const newRule: AutoReplyRule = {
      id: makeId(),
      label: undefined,
      triggerType: "data",
      timerIntervalMs: 1000,
      conditions: [{ pattern: "", mode: "contains", caseSensitive: false, negate: false }],
      conditionLogic: "and",
      actions: [],
      enabled: true,
      cooldownMs: 0,
    };
    setEditingRule(newRule);
    setEditorOpen(true);
  }, []);

  const handleEditRule = useCallback((rule: AutoReplyRule) => {
    setDeleteConfirmId(null);
    setEditingRule({ ...rule });
    setEditorOpen(true);
  }, []);

  const handleSaveRule = useCallback((rule: AutoReplyRule) => {
    const updated = rules.map(r => r.id === rule.id ? rule : r);
    const exists = rules.some(r => r.id === rule.id);
    const final = exists ? updated : [...rules, rule];
    persistRules(final);
    setEditorOpen(false);
    setEditingRule(null);
  }, [rules, persistRules]);

  const handleToggleRule = useCallback((ruleId: string) => {
    setDeleteConfirmId(null);
    persistRules(rules.map(r =>
      r.id === ruleId ? { ...r, enabled: !r.enabled } : r
    ));
  }, [rules, persistRules]);

  // 全选/取消全选 = 启用/停用全部规则（规则复选框即持久 enabled）
  const handleSelectAllRules = useCallback(() => {
    const allEnabled = rules.length > 0 && rules.every(r => r.enabled);
    persistRules(rules.map(r => ({ ...r, enabled: !allEnabled })));
  }, [rules, persistRules]);

  const handleDeleteRule = useCallback((ruleId: string) => {
    setDeleteConfirmId(ruleId);
  }, []);

  const confirmDeleteRule = useCallback((ruleId: string) => {
    persistRules(rules.filter(r => r.id !== ruleId));
    setDeleteConfirmId(null);
  }, [rules, persistRules]);

  // ── 执行控制 ──
  const handleStart = useCallback(async () => {
    if (!isConnected) return;
    setIsLoading(true);
    try {
      const code: string = await invoke("rules_to_script", {
        rules: rules.filter(r => r.enabled),
        name: activeConfigName,
        matchStrategy: matchStrategy,
      });
      await invoke("start_script_engine", { sessionId, code });
      dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: true });
      onRunningChange?.(true);
    } catch (e) {
      console.error("Failed to start auto-reply:", e);
      showToast("error", `${t("sendBar.startFailed")}: ${e}`);
    } finally {
      setIsLoading(false);
    }
  }, [isConnected, rules, activeConfigName, matchStrategy, sessionId, dispatch, onRunningChange, showToast, t]);

  const handleStop = useCallback(async () => {
    try {
      await invoke("stop_script_engine", { sessionId });
      dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: false });
      onRunningChange?.(false);
    } catch (e) {
      console.error("Failed to stop auto-reply:", e);
      showToast("error", `${t("sendBar.stopFailed")}: ${e}`);
    }
  }, [sessionId, dispatch, onRunningChange, showToast, t]);

  // ── 转换为脚本 ──
  const handleConvertToScript = useCallback(async () => {
    try {
      const code: string = await invoke("rules_to_script", {
        rules: rules.filter(r => r.enabled),
        name: activeConfigName,
        matchStrategy: matchStrategy,
      });
      // 自动创建脚本条目，确保编辑器可渲染（ScriptEditor 在 scripts.length === 0 时不渲染编辑器）
      const newScript: ScriptRecord = {
        id: crypto.randomUUID(),
        name: activeConfigName,
        code,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      };
      const updatedScripts = [...scripts, newScript];
      localStorage.setItem("tauterm-scripts", JSON.stringify(updatedScripts));
      localStorage.setItem("tauterm-active-script-id", newScript.id);
      dispatch({ type: "SET_SCRIPTS", scripts: updatedScripts });
      dispatch({ type: "SET_ACTIVE_SCRIPT", id: newScript.id });
      // 加载生成的代码并切换到脚本模式
      dispatch({ type: "SET_SCRIPT_CODE", code });
      dispatch({ type: "SET_MODE", mode: "script" });
    } catch (e) {
      console.error("Failed to convert to script:", e);
      showToast("error", `${t("sendBar.convertFailed")}: ${e}`);
    }
  }, [rules, scripts, activeConfigName, matchStrategy, dispatch, showToast, t]);

  // ── 导入/导出 ──
  const handleExport = useCallback(() => {
    const config = activeConfig;
    if (!config) return;
    const json = JSON.stringify(config, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(config.name || "config").replace(/\s+/g, "_")}.tauterm-reply.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [activeConfig]);

  const handleImport = useCallback(() => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const parsed = JSON.parse(text);
        if (!Array.isArray(parsed.rules)) {
          throw new Error("Invalid config format: missing rules");
        }
        setImportData(parsed as AutoReplyConfig);
        setImportOpen(true);
      } catch (err) {
        showToast("error", `${t("sendBar.importFailed")}: ${err}`);
      }
    };
    input.click();
  }, [showToast, t]);

  const handleImportOverwrite = useCallback(() => {
    if (!importData) return;
    const updated = configs.map(c =>
      c.name === activeConfigName ? { ...importData, name: activeConfigName } : c
    );
    persist(updated);
    dispatch({ type: "SET_AUTO_REPLY_RULES", rules: importData.rules });
    setImportOpen(false);
    setImportData(null);
    showToast("success", t("sendBar.importSuccess"));
  }, [importData, activeConfigName, configs, persist, dispatch, showToast, t]);

  const handleImportAppend = useCallback(() => {
    if (!importData) return;
    const newName = `${importData.name || "Imported"} (${t("sendBar.imported")})`;
    const updated = [...configs, { ...importData, name: newName }];
    persist(updated);
    persistActive(newName);
    setImportOpen(false);
    setImportData(null);
    showToast("success", t("sendBar.importSuccess"));
  }, [importData, configs, persist, persistActive, showToast, t]);

  // ── 加载内置示例 ──
  const handleLoadExamples = useCallback(() => {
    const existingNames = new Set(configs.map(c => c.name));
    const newBuiltins = BUILTIN_CONFIGS.filter(c => !existingNames.has(c.name));
    if (newBuiltins.length === 0) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    persist([...configs, ...newBuiltins]);
    showToast("success", t("sendBar.examplesLoaded", { count: newBuiltins.length }));
  }, [configs, persist, showToast, t]);

  const enabledCount = rules.filter(r => r.enabled).length;

  return (
    <div className={styles.panel}>
      {/* 配置工具栏 */}
      <div className={styles.toolbar}>
        {/* 左侧：配置管理 */}
        <div className={styles.configActions}>
          <select
            className={`${styles.configSelect} liquid-glass-input liquid-glass-select`}
            value={activeConfigName}
            onChange={e => handleSelectConfig(e.target.value)}
          >
            {configs.map(c => (
              <option key={c.name} value={c.name}>{c.name}</option>
            ))}
          </select>
          {/* 匹配策略 — 与配置关联 */}
          <span className={styles.toolbarDivider} />
          <select
            className={`${styles.strategyDropdown} liquid-glass-input liquid-glass-select`}
            value={matchStrategy}
            onChange={e => {
              const strategy = e.target.value as MatchStrategy;
              dispatch({ type: "SET_MATCH_STRATEGY", strategy });
              // write back to active config
              const updatedConfigs = configs.map(c =>
                c.name === activeConfigName ? { ...c, matchStrategy: strategy } : c
              );
              persist(updatedConfigs);
            }}
            title={t("sendBar.matchStrategy")}
          >
            <option value="all">{t("sendBar.matchStrategyAll")}</option>
            <option value="first">{t("sendBar.matchStrategyFirst")}</option>
          </select>
          <span className={styles.toolbarDivider} />
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleNewConfig} title={t("sendBar.new")}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleRenameConfig} title={t("sendBar.rename")}>
            <Icon name="edit" size="sm" />
          </button>
          {configDeleteConfirm ? (
            <div className={styles.toolConfirm}>
              <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleDeleteConfig}
                title={t("sendBar.confirmDeleteHint")}>
                <Icon name="warning" size="sm" />
                <span className={styles.deleteHint}>{t("sendBar.confirmDeleteHint")}</span>
              </button>
              <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => setConfigDeleteConfirm(false)}
                title={t("sendBar.cancel")}>
                <Icon name="close" size="sm" />
              </button>
            </div>
          ) : (
            <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleDeleteConfig} disabled={configs.length === 0}
              title={t("sendBar.delete")}>
              <Icon name="trash" size="sm" />
            </button>
          )}
        </div>
        {/* 右侧：操作按钮 */}
        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleLoadExamples}
            title={t("sendBar.loadBuiltinExamples")}>
            {t("sendBar.loadBuiltinExamples")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExport} disabled={!activeConfig}
            title={t("sendBar.exportConfig")}>
            {t("sendBar.exportConfig")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleImport}
            title={t("sendBar.importConfig")}>
            {t("sendBar.importConfig")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleAddRule}>
            + {t("sendBar.addRule")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleConvertToScript} disabled={enabledCount === 0}>
            {t("sendBar.convertToScript")}
          </button>
        </div>
      </div>

      {/* 规则列表 */}
      {configs.length === 0 ? (
        <div className={styles.ruleList}>
          <div className={styles.empty}>{t("sendBar.noConfigs")}</div>
        </div>
      ) : (
      <div
        ref={listRef}
        className={`${styles.ruleList} ${isDragging ? styles.listDragging : ""}`}
      >
        {rules.length === 0 && (
          <div className={styles.empty}>{t("sendBar.noRules")}</div>
        )}
        {rules.map((rule, i) => {
          const confirming = deleteConfirmId === rule.id;
          const seqSummary = rule.actions.map((a) => a.data).filter(Boolean).join(" › ");
          return (
            <Fragment key={rule.id}>
              {isDragging && dropIndex === i && (
                <div className={styles.dropIndicator} />
              )}
              <div className={`${styles.ruleRow} ${rule.enabled ? styles.rowEnabled : ""} ${confirming ? styles.rowConfirming : ""}`}>
                <span
                  className={styles.dragHandle}
                  title={t("commandPanel.dragToReorder")}
                  onPointerDown={(e) => handlePointerDown(e, i)}
                  onPointerMove={handlePointerMove}
                  onPointerUp={handlePointerUp}
                  onPointerCancel={handlePointerCancel}
                  style={{ touchAction: 'none' }}
                >
                  <Icon name="drag-handle" size={16} color="currentColor" />
                </span>
                <label className={styles.checkLabel}>
                  <input
                    type="checkbox"
                    className={styles.checkInput}
                    checked={rule.enabled}
                    onChange={() => handleToggleRule(rule.id)}
                  />
                  <div className={styles.checkTrack} />
                </label>
                <code className={styles.rulePatternText} title={rule.triggerType === "timer" ? `Timer ${rule.timerIntervalMs}ms` : (rule.conditions.length > 0 ? rule.conditions.map(c => c.pattern).join(rule.conditionLogic === "or" ? " | " : " & ") : "(empty)")}>
                  {rule.triggerType === "timer"
                    ? `⏱ ${rule.timerIntervalMs}ms`
                    : rule.conditions.length > 0
                      ? rule.conditions.map(c => (c.negate ? "!" : "") + (c.pattern || "(empty)")).join(rule.conditionLogic === "or" ? " | " : " & ")
                      : "(empty)"}
                </code>
                {rule.triggerType !== "timer" && rule.conditions.length > 0 && (
                  <span className={styles.matchModeBadge}>
                    {rule.conditions.length > 1
                      ? (rule.conditionLogic === "or" ? "OR" : "AND")
                      : t("sendBar." + (MATCH_MODE_KEY[rule.conditions[0].mode] || "matchContains"))}
                    {rule.conditions.length === 1 && rule.conditions[0].caseSensitive && <span className={styles.caseSensitiveMark}>Aa</span>}
                  </span>
                )}
                {rule.conditions.some(c => c.matchFormat === "hex") && (
                  <span className={styles.ruleBadge} title={t("sendBar.matchFormatHex")}>HEX</span>
                )}
                {rule.triggerType === "timer" && (
                  <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={t("sendBar.triggerTypeTimer")}>
                    <Icon name="stopwatch" size="xs" color="currentColor" />
                  </span>
                )}
                {rule.triggerType !== "timer" && rule.conditions.length === 1 && rule.conditions[0].mode === "regex" && (
                  <span className={styles.ruleBadge} title={t("sendBar.matchRegex")}>.*</span>
                )}
                {rule.actions.some(a => a.data.includes("{{")) && (
                  <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title="Macros">
                    <Icon name="code" size="xs" color="currentColor" />
                  </span>
                )}
                {rule.actions.length > 0 && (
                  <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={t("sendBar.sequenceSummary", { count: rule.actions.length })}>
                    <Icon name="log" size="xs" color="currentColor" />
                    {rule.actions.length}
                  </span>
                )}
                {rule.cooldownMs > 0 && (
                  <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={`${t("sendBar.cooldownMs")}: ${rule.cooldownMs}ms`}>
                    <Icon name="stopwatch" size="xs" color="currentColor" />
                  </span>
                )}
                <code
                  className={styles.ruleReplyText}
                  title={seqSummary || t("sendBar.sequenceSummary", { count: rule.actions.length })}
                >
                  {seqSummary || t("sendBar.sequenceSummary", { count: rule.actions.length })}
                </code>
                <div className={styles.ruleSpacer} />
                <span className={styles.ruleLabelText}>{rule.label?.trim() || ""}</span>
                <button className={`${styles.editBtn} liquid-glass-button`} onClick={() => handleEditRule(rule)} title={t("sendBar.edit")}>
                  <Icon name="edit" size="sm" />
                </button>
                {confirming ? (
                  <div className={styles.confirmBox}>
                    <span className={styles.confirmText}>{t("commandPanel.confirmDelete")}</span>
                    <button className={`${styles.confirmBtn} liquid-glass-button`} onClick={() => confirmDeleteRule(rule.id)}>
                      {t("common.confirm")}
                    </button>
                    <button className={`${styles.confirmBtn} liquid-glass-button`} onClick={() => setDeleteConfirmId(null)}>
                      {t("common.cancel")}
                    </button>
                  </div>
                ) : (
                  <button className={`${styles.deleteBtn} liquid-glass-button`} onClick={() => handleDeleteRule(rule.id)} title={t("sendBar.delete")}>
                    <Icon name="trash" size="sm" />
                  </button>
                )}
              </div>
            </Fragment>
          );
        })}
        {isDragging && dropIndex === rules.length && rules.length > 0 && (
          <div className={styles.dropIndicator} />
        )}
      </div>
      )}

      {/* 脚本输出面板 — 始终可见（日志由 Provider 层始终监听） */}
      <div className={`${styles.logPanel} ${logExpanded ? "" : styles.logPanelCollapsed}`}>
        <div className={styles.logPanelHeader} onClick={() => setLogExpanded(!logExpanded)}>
          <span className={styles.logPanelTitle}>
            {t("sendBar.scriptOutput")}
            {scriptLogs.length > 0 && ` (${scriptLogs.length})`}
          </span>
          <div className={styles.logPanelActions}>
            {scriptLogs.length > 0 && (
              <button
                className={`${styles.logClearBtn} liquid-glass-button`}
                onClick={(e) => { e.stopPropagation(); dispatch({ type: "CLEAR_SCRIPT_LOGS" }); }}
                title={t("sendBar.clearOutput")}
              >
                <Icon name="trash" size="xs" />
              </button>
            )}
            <Icon name={logExpanded ? "chevron-down" : "chevron-right"} size="sm" />
          </div>
        </div>
        {logExpanded && (
          <div className={styles.logPanelContent}>
            {scriptLogs.length === 0 && (
              <div className={styles.logPanelEmpty}>{t("sendBar.noOutput")}</div>
            )}
            {scriptLogs.map((msg, i) => (
              <div key={i} className={styles.logPanelLine}>{msg}</div>
            ))}
          </div>
        )}
      </div>

      {/* 执行控制 */}
      <div className={styles.controls}>
        <span className={styles.status}>
          <Icon name={isRunning ? "status-connected" : "status-idle"} size={10} />
          {isRunning
            ? `${t("sendBar.running")} · ${enabledCount} ${t("sendBar.rulesActive")}`
            : enabledCount > 0
              ? `${enabledCount} ${t("sendBar.rulesEnabled")}`
              : t("sendBar.idle")}
        </span>
        <label className={styles.controlLabel}>
          <input
            type="checkbox"
            className={styles.checkInput}
            checked={rules.length > 0 && rules.every(r => r.enabled)}
            onChange={handleSelectAllRules}
            disabled={rules.length === 0}
          />
          <div className={styles.checkTrack} />
          <span>{t("commandPanel.selectAll")}</span>
        </label>
        <div className={styles.controlBtns}>
          {!isRunning ? (
            <button className={`${styles.startBtn} liquid-primary-button`} onClick={handleStart} disabled={!isConnected || enabledCount === 0 || isLoading}>
              <Icon name="play" size="xs" /> {t("commandPanel.start")}
            </button>
          ) : (
            <button className={styles.stopBtn} onClick={handleStop}>
              <Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}
            </button>
          )}
        </div>
      </div>

      {/* 规则编辑弹窗 */}
      {editorOpen && editingRule && (
        <AutoReplyRuleEditor
          rule={editingRule}
          onSave={handleSaveRule}
          onCancel={() => { setEditorOpen(false); setEditingRule(null); }}
        />
      )}

      {/* 重命名弹窗 */}
      {renameOpen && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={handleCancelRename}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={e => e.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.renameTitle")}</h3>
            <input
              className={`${styles.renameInput} liquid-glass-input`}
              type="text"
              value={renameValue}
              onChange={e => setRenameValue(e.target.value)}
              onKeyDown={e => {
                if (e.key === "Enter") handleConfirmRename();
                else if (e.key === "Escape") handleCancelRename();
              }}
              placeholder={t("sendBar.renamePlaceholder")}
              autoFocus
            />
            <div className={styles.renameBtns}>
              <button className={`${styles.renameCancelBtn} liquid-glass-button`} onClick={handleCancelRename}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-primary-button`} onClick={handleConfirmRename} disabled={!renameValue.trim()}>
                {t("sendBar.save")}
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}

      {/* 导入确认弹窗 */}
      {importOpen && importData && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={() => { setImportOpen(false); setImportData(null); }}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={e => e.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.importConfirmTitle")}</h3>
            <p className={styles.importInfo}>
              {t("sendBar.importName")}: {importData.name}<br/>
              {t("sendBar.importRules")}: {importData.rules.length}
            </p>
            <div className={styles.renameBtns}>
              <button className={`${styles.renameCancelBtn} liquid-glass-button`}
                onClick={() => { setImportOpen(false); setImportData(null); }}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-glass-button`} onClick={handleImportAppend}>
                {t("sendBar.importAppend")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-primary-button`} onClick={handleImportOverwrite}>
                {t("sendBar.importOverwrite")}
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}
    </div>
  );
}
