import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { usePointerDragReorder } from "../../hooks/usePointerDragReorder";
import ConfirmDialog from "../common/ConfirmDialog";
import Icon from "../common/Icon";
import AutoReplyRuleEditor from "./AutoReplyRuleEditor";
import { useSendBar } from "./SendBarContext";
import type { AutoReplyRule, AutoReplyConfig, MatchStrategy, ScriptRecord } from "./types";
import { BUILTIN_CONFIGS } from "./builtinRules";
import { ASSET_KEYS, clearAsset, persistAsset } from "./assetStore";
import { parseAutoReplyConfig, uniqueAssetName } from "./assetValidation";
import styles from "./AutoReplyPanel.module.css";

interface AutoReplyPanelProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

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

function defaultConfig(name: string): AutoReplyConfig {
  return { name, matchStrategy: "all", rules: [] };
}

export default function AutoReplyPanel({ sessionId, isActive, onRunningChange }: AutoReplyPanelProps) {
  const { t } = useTranslation();
  const { isSessionConnected } = useSession();
  const { showToast } = useToast();
  const isConnected = isSessionConnected(sessionId);

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
  const runtimeLocked = isRunning || isLoading;

  const activeConfig = useMemo(
    () => configs.find(config => config.name === activeConfigName) ?? configs[0],
    [configs, activeConfigName],
  );
  const deletingRule = useMemo(
    () => rules.find(rule => rule.id === deleteConfirmId) ?? null,
    [rules, deleteConfirmId],
  );

  // Runtime uses a generated Lua snapshot. While starting/running, keep the visible rule set frozen.
  useEffect(() => {
    if (!activeConfig || runtimeLocked) return;
    if (activeConfig.name !== activeConfigName) {
      dispatch({ type: "SET_ACTIVE_AUTO_REPLY_CONFIG", name: activeConfig.name });
    }
    dispatch({ type: "SET_AUTO_REPLY_RULES", rules: activeConfig.rules });
    dispatch({ type: "SET_MATCH_STRATEGY", strategy: activeConfig.matchStrategy });
  }, [activeConfigName, activeConfig, runtimeLocked, dispatch]);

  useEffect(() => {
    setConfigDeleteConfirm(false);
    setDeleteConfirmId(null);
  }, [activeConfigName]);

  useEffect(() => {
    const unlisten = listen<{ session_id: string }>("session-disconnected", event => {
      if (event.payload.session_id === sessionId && isRunning) {
        dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: false });
        onRunningChange?.(false);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, isRunning, dispatch, onRunningChange]);

  useEffect(() => {
    const lastMsg = scriptLogs[scriptLogs.length - 1];
    if (lastMsg && (lastMsg.includes("失败") || lastMsg.includes("错误") || lastMsg.includes("Error"))) {
      if (isActive && isRunning) showToast("error", lastMsg);
    }
  }, [scriptLogs, isActive, isRunning, showToast]);

  const persist = useCallback((updated: AutoReplyConfig[]) => {
    dispatch({ type: "SET_AUTO_REPLY_CONFIGS", configs: updated });
    return persistAsset(ASSET_KEYS.autoReplyConfigs, updated);
  }, [dispatch]);

  const persistActive = useCallback((name: string) => {
    dispatch({ type: "SET_ACTIVE_AUTO_REPLY_CONFIG", name });
    return name
      ? persistAsset(ASSET_KEYS.activeAutoReplyConfig, name)
      : clearAsset(ASSET_KEYS.activeAutoReplyConfig);
  }, [dispatch]);

  const persistRules = useCallback((updated: AutoReplyRule[]) => {
    if (runtimeLocked) return;
    dispatch({ type: "SET_AUTO_REPLY_RULES", rules: updated });
    const updatedConfigs = configs.map(config =>
      config.name === activeConfigName ? { ...config, rules: updated } : config,
    );
    void persist(updatedConfigs);
  }, [runtimeLocked, configs, activeConfigName, dispatch, persist]);

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
    disabled: runtimeLocked,
    listRef,
  });

  // ── 配置管理 ──
  const handleSelectConfig = useCallback((name: string) => {
    if (!runtimeLocked) void persistActive(name);
  }, [runtimeLocked, persistActive]);

  const handleNewConfig = useCallback(() => {
    if (runtimeLocked) return;
    const existing = new Set(configs.map(config => config.name));
    let index = configs.length + 1;
    let name = t("sendBar.newConfigName", { n: index });
    while (existing.has(name)) {
      index += 1;
      name = t("sendBar.newConfigName", { n: index });
    }
    void persist([...configs, defaultConfig(name)]);
    void persistActive(name);
  }, [runtimeLocked, configs, persist, persistActive, t]);

  const handleRenameConfig = useCallback(() => {
    if (runtimeLocked || !activeConfig) return;
    setRenameValue(activeConfig.name);
    setRenameOpen(true);
  }, [runtimeLocked, activeConfig]);

  const handleConfirmRename = useCallback(() => {
    if (runtimeLocked) return;
    const newName = renameValue.trim();
    if (!newName || newName === activeConfigName) {
      setRenameOpen(false);
      return;
    }
    if (configs.some(config => config.name === newName)) {
      showToast("error", t("sendBar.nameExists", { defaultValue: "Name already exists" }));
      return;
    }
    const updated = configs.map(config =>
      config.name === activeConfigName ? { ...config, name: newName } : config,
    );
    void persist(updated);
    void persistActive(newName);
    setRenameOpen(false);
  }, [runtimeLocked, renameValue, activeConfigName, configs, persist, persistActive, showToast, t]);

  const handleCancelRename = useCallback(() => setRenameOpen(false), []);

  const handleDeleteConfig = useCallback(() => {
    if (runtimeLocked || configs.length === 0) return;
    const updated = configs.filter(config => config.name !== activeConfigName);
    void persist(updated);
    void persistActive(updated[0]?.name || "");
    if (updated.length === 0) {
      dispatch({ type: "SET_AUTO_REPLY_RULES", rules: [] });
    }
    setConfigDeleteConfirm(false);
  }, [runtimeLocked, activeConfigName, configs, persist, persistActive, dispatch]);

  // ── 规则管理 ──
  const handleAddRule = useCallback(() => {
    if (runtimeLocked) return;
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
  }, [runtimeLocked]);

  const handleEditRule = useCallback((rule: AutoReplyRule) => {
    if (runtimeLocked) return;
    setDeleteConfirmId(null);
    setEditingRule({ ...rule });
    setEditorOpen(true);
  }, [runtimeLocked]);

  const handleSaveRule = useCallback((rule: AutoReplyRule) => {
    if (runtimeLocked) return;
    const exists = rules.some(existing => existing.id === rule.id);
    const next = exists ? rules.map(existing => existing.id === rule.id ? rule : existing) : [...rules, rule];
    persistRules(next);
    setEditorOpen(false);
    setEditingRule(null);
  }, [runtimeLocked, rules, persistRules]);

  const handleToggleRule = useCallback((ruleId: string) => {
    if (runtimeLocked) return;
    setDeleteConfirmId(null);
    persistRules(rules.map(rule =>
      rule.id === ruleId ? { ...rule, enabled: !rule.enabled } : rule,
    ));
  }, [runtimeLocked, rules, persistRules]);

  const handleSelectAllRules = useCallback(() => {
    if (runtimeLocked) return;
    const allEnabled = rules.length > 0 && rules.every(rule => rule.enabled);
    persistRules(rules.map(rule => ({ ...rule, enabled: !allEnabled })));
  }, [runtimeLocked, rules, persistRules]);

  const confirmDeleteRule = useCallback(() => {
    if (runtimeLocked || !deleteConfirmId) return;
    persistRules(rules.filter(rule => rule.id !== deleteConfirmId));
    setDeleteConfirmId(null);
  }, [runtimeLocked, deleteConfirmId, rules, persistRules]);

  // ── 执行控制 ──
  const handleStart = useCallback(async () => {
    if (runtimeLocked || !isConnected) return;

    // Lock the parent mode before the first async boundary. Otherwise the panel could unmount while
    // rules_to_script/start_script_engine is still pending and later report a stale running state.
    setIsLoading(true);
    onRunningChange?.(true);
    setEditorOpen(false);
    setEditingRule(null);
    setRenameOpen(false);
    setImportOpen(false);
    setImportData(null);
    setDeleteConfirmId(null);
    setConfigDeleteConfirm(false);

    try {
      const code: string = await invoke("rules_to_script", {
        rules: rules.filter(rule => rule.enabled),
        name: activeConfigName,
        matchStrategy,
      });
      await invoke("start_script_engine", { sessionId, code });
      dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: true });
    } catch (e) {
      onRunningChange?.(false);
      console.error("Failed to start auto-reply:", e);
      showToast("error", `${t("sendBar.startFailed")}: ${String(e)}`);
    } finally {
      setIsLoading(false);
    }
  }, [runtimeLocked, isConnected, rules, activeConfigName, matchStrategy, sessionId, dispatch, onRunningChange, showToast, t]);

  const handleStop = useCallback(async () => {
    if (!isRunning || isLoading) return;
    setIsLoading(true);
    try {
      await invoke("stop_script_engine", { sessionId });
      dispatch({ type: "SET_AUTO_REPLY_RUNNING", running: false });
      onRunningChange?.(false);
    } catch (e) {
      console.error("Failed to stop auto-reply:", e);
      showToast("error", `${t("sendBar.stopFailed")}: ${String(e)}`);
    } finally {
      setIsLoading(false);
    }
  }, [sessionId, isRunning, isLoading, dispatch, onRunningChange, showToast, t]);

  // ── 转换为脚本 ──
  const handleConvertToScript = useCallback(async () => {
    if (runtimeLocked) return;
    try {
      const code: string = await invoke("rules_to_script", {
        rules: rules.filter(rule => rule.enabled),
        name: activeConfigName,
        matchStrategy,
      });
      const newScript: ScriptRecord = {
        id: crypto.randomUUID(),
        name: uniqueAssetName(activeConfigName || "Auto Reply", scripts.map(script => script.name), t("sendBar.imported")),
        code,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      };
      const updatedScripts = [...scripts, newScript];
      if (!await persistAsset(ASSET_KEYS.scripts, updatedScripts)) return;
      if (!await persistAsset(ASSET_KEYS.activeScriptId, newScript.id)) return;
      dispatch({ type: "SET_SCRIPTS", scripts: updatedScripts });
      dispatch({ type: "SET_ACTIVE_SCRIPT", id: newScript.id });
      dispatch({ type: "SET_SCRIPT_CODE", code });
      dispatch({ type: "SET_MODE", mode: "script" });
    } catch (e) {
      console.error("Failed to convert to script:", e);
      showToast("error", `${t("sendBar.convertFailed")}: ${String(e)}`);
    }
  }, [runtimeLocked, rules, scripts, activeConfigName, matchStrategy, dispatch, showToast, t]);

  // ── 导入/导出 ──
  const handleExport = useCallback(() => {
    if (runtimeLocked) return;
    const config = activeConfig;
    if (!config) return;
    const json = JSON.stringify(config, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(config.name || "config").replace(/\s+/g, "_")}.tauterm-reply.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [runtimeLocked, activeConfig]);

  const handleImport = useCallback(() => {
    if (runtimeLocked) return;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async event => {
      const file = (event.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        setImportData(parseAutoReplyConfig(JSON.parse(await file.text())));
        setImportOpen(true);
      } catch (err) {
        showToast("error", `${t("sendBar.importFailed")}: ${String(err)}`);
      }
    };
    input.click();
  }, [runtimeLocked, showToast, t]);

  const handleImportOverwrite = useCallback(async () => {
    if (runtimeLocked || !importData || !activeConfig) return;
    const updated = configs.map(config =>
      config.name === activeConfigName ? { ...importData, name: activeConfigName } : config,
    );
    const saved = await persist(updated);
    dispatch({ type: "SET_AUTO_REPLY_RULES", rules: importData.rules });
    dispatch({ type: "SET_MATCH_STRATEGY", strategy: importData.matchStrategy });
    setImportOpen(false);
    setImportData(null);
    if (saved) showToast("success", t("sendBar.importSuccess"));
  }, [runtimeLocked, importData, activeConfig, activeConfigName, configs, persist, dispatch, showToast, t]);

  const handleImportAppend = useCallback(async () => {
    if (runtimeLocked || !importData) return;
    const newName = uniqueAssetName(importData.name, configs.map(config => config.name), t("sendBar.imported"));
    const updated = [...configs, { ...importData, name: newName }];
    const savedConfigs = await persist(updated);
    if (!savedConfigs) return;
    const savedActive = await persistActive(newName);
    setImportOpen(false);
    setImportData(null);
    if (savedActive) showToast("success", t("sendBar.importSuccess"));
  }, [runtimeLocked, importData, configs, persist, persistActive, showToast, t]);

  const handleLoadExamples = useCallback(async () => {
    if (runtimeLocked) return;
    const existingNames = new Set(configs.map(config => config.name));
    const newBuiltins = BUILTIN_CONFIGS.filter(config => !existingNames.has(config.name));
    if (newBuiltins.length === 0) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    if (await persist([...configs, ...newBuiltins])) {
      showToast("success", t("sendBar.examplesLoaded", { count: newBuiltins.length }));
    }
  }, [runtimeLocked, configs, persist, showToast, t]);

  const enabledCount = rules.filter(rule => rule.enabled).length;

  return (
    <div className={styles.panel} aria-busy={isLoading || undefined}>
      <div className={styles.toolbar}>
        <div className={styles.configActions}>
          <select
            className={`${styles.configSelect} liquid-glass-input liquid-glass-select`}
            value={activeConfigName}
            onChange={event => handleSelectConfig(event.target.value)}
            disabled={runtimeLocked}
          >
            {configs.map(config => (
              <option key={config.name} value={config.name}>{config.name}</option>
            ))}
          </select>
          <span className={styles.toolbarDivider} />
          <select
            className={`${styles.strategyDropdown} liquid-glass-input liquid-glass-select`}
            value={matchStrategy}
            onChange={event => {
              if (runtimeLocked) return;
              const strategy = event.target.value as MatchStrategy;
              dispatch({ type: "SET_MATCH_STRATEGY", strategy });
              void persist(configs.map(config =>
                config.name === activeConfigName ? { ...config, matchStrategy: strategy } : config,
              ));
            }}
            title={t("sendBar.matchStrategy")}
            disabled={runtimeLocked}
          >
            <option value="all">{t("sendBar.matchStrategyAll")}</option>
            <option value="first">{t("sendBar.matchStrategyFirst")}</option>
          </select>
          <span className={styles.toolbarDivider} />
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleNewConfig} title={t("sendBar.new")} disabled={runtimeLocked}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleRenameConfig} title={t("sendBar.rename")} disabled={runtimeLocked || !activeConfig}>
            <Icon name="edit" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => setConfigDeleteConfirm(true)} disabled={runtimeLocked || configs.length === 0} title={t("sendBar.delete")}>
            <Icon name="trash" size="sm" />
          </button>
        </div>
        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleLoadExamples(); }} title={t("sendBar.loadBuiltinExamples")} disabled={runtimeLocked}>
            {t("sendBar.loadBuiltinExamples")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExport} disabled={runtimeLocked || !activeConfig} title={t("sendBar.exportConfig")}>
            {t("sendBar.exportConfig")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleImport} title={t("sendBar.importConfig")} disabled={runtimeLocked}>
            {t("sendBar.importConfig")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleAddRule} disabled={runtimeLocked}>
            + {t("sendBar.addRule")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleConvertToScript(); }} disabled={runtimeLocked || enabledCount === 0}>
            {t("sendBar.convertToScript")}
          </button>
        </div>
      </div>

      {configs.length === 0 ? (
        <div className={styles.ruleList}>
          <div className={styles.empty}>{t("sendBar.noConfigs")}</div>
        </div>
      ) : (
        <div ref={listRef} className={`${styles.ruleList} ${isDragging ? styles.listDragging : ""}`}>
          {rules.length === 0 && <div className={styles.empty}>{t("sendBar.noRules")}</div>}
          {rules.map((rule, index) => {
            const seqSummary = rule.actions.map(action => action.data).filter(Boolean).join(" › ");
            const conditionSummary = rule.conditions.length > 0
              ? rule.conditions.map(condition => condition.pattern).join(rule.conditionLogic === "or" ? " | " : " & ")
              : "(empty)";
            return (
              <Fragment key={rule.id}>
                {isDragging && dropIndex === index && <div className={styles.dropIndicator} />}
                <div className={`${styles.ruleRow} ${rule.enabled ? styles.rowEnabled : ""}`}>
                  <span
                    className={styles.dragHandle}
                    title={t("commandPanel.dragToReorder")}
                    onPointerDown={event => handlePointerDown(event, index)}
                    onPointerMove={handlePointerMove}
                    onPointerUp={handlePointerUp}
                    onPointerCancel={handlePointerCancel}
                    style={{ touchAction: "none" }}
                  >
                    <Icon name="drag-handle" size={16} />
                  </span>
                  <label className={styles.checkLabel}>
                    <input
                      type="checkbox"
                      className={styles.checkInput}
                      checked={rule.enabled}
                      onChange={() => handleToggleRule(rule.id)}
                      disabled={runtimeLocked}
                    />
                    <div className={styles.checkTrack} />
                  </label>
                  <code className={styles.rulePatternText} title={rule.triggerType === "timer" ? `Timer ${rule.timerIntervalMs}ms` : conditionSummary}>
                    {rule.triggerType === "timer"
                      ? <><Icon name="stopwatch" size="xs" /> {rule.timerIntervalMs}ms</>
                      : rule.conditions.length > 0
                        ? rule.conditions.map(condition => (condition.negate ? "!" : "") + (condition.pattern || "(empty)")).join(rule.conditionLogic === "or" ? " | " : " & ")
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
                  {rule.conditions.some(condition => condition.matchFormat === "hex") && (
                    <span className={styles.ruleBadge} title={t("sendBar.matchFormatHex")}>HEX</span>
                  )}
                  {rule.triggerType === "timer" && (
                    <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={t("sendBar.triggerTypeTimer")}>
                      <Icon name="stopwatch" size="xs" />
                    </span>
                  )}
                  {rule.triggerType !== "timer" && rule.conditions.length === 1 && rule.conditions[0].mode === "regex" && (
                    <span className={styles.ruleBadge} title={t("sendBar.matchRegex")}>.*</span>
                  )}
                  {rule.actions.some(action => action.data.includes("{{")) && (
                    <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title="Macros">
                      <Icon name="code" size="xs" />
                    </span>
                  )}
                  {rule.actions.length > 0 && (
                    <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={t("sendBar.sequenceSummary", { count: rule.actions.length })}>
                      <Icon name="steps" size="xs" />
                      {rule.actions.length}
                    </span>
                  )}
                  {rule.cooldownMs > 0 && (
                    <span className={`${styles.ruleBadge} ${styles.ruleBadgeIcon}`} title={`${t("sendBar.cooldownMs")}: ${rule.cooldownMs}ms`}>
                      <Icon name="stopwatch" size="xs" />
                    </span>
                  )}
                  <code className={styles.ruleReplyText} title={seqSummary || t("sendBar.sequenceSummary", { count: rule.actions.length })}>
                    {seqSummary || t("sendBar.sequenceSummary", { count: rule.actions.length })}
                  </code>
                  <div className={styles.ruleSpacer} />
                  <span className={styles.ruleLabelText}>{rule.label?.trim() || ""}</span>
                  <button className={`${styles.editBtn} liquid-glass-button`} onClick={() => handleEditRule(rule)} title={t("sendBar.edit")} disabled={runtimeLocked}>
                    <Icon name="edit" size="sm" />
                  </button>
                  <button className={`${styles.deleteBtn} liquid-glass-button`} onClick={() => setDeleteConfirmId(rule.id)} title={t("sendBar.delete")} disabled={runtimeLocked}>
                    <Icon name="trash" size="sm" />
                  </button>
                </div>
              </Fragment>
            );
          })}
          {isDragging && dropIndex === rules.length && rules.length > 0 && <div className={styles.dropIndicator} />}
        </div>
      )}

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
                onClick={event => { event.stopPropagation(); dispatch({ type: "CLEAR_SCRIPT_LOGS" }); }}
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
            {scriptLogs.length === 0 && <div className={styles.logPanelEmpty}>{t("sendBar.noOutput")}</div>}
            {scriptLogs.map((message, index) => (
              <div key={index} className={styles.logPanelLine}>{message}</div>
            ))}
          </div>
        )}
      </div>

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
            checked={rules.length > 0 && rules.every(rule => rule.enabled)}
            onChange={handleSelectAllRules}
            disabled={runtimeLocked || rules.length === 0}
          />
          <div className={styles.checkTrack} />
          <span>{t("commandPanel.selectAll")}</span>
        </label>
        <div className={styles.controlBtns}>
          {!isRunning ? (
            <button className={`${styles.startBtn} liquid-primary-button`} onClick={() => { void handleStart(); }} disabled={runtimeLocked || !isConnected || enabledCount === 0}>
              <Icon name="play" size="xs" /> {t("commandPanel.start")}
            </button>
          ) : (
            <button className={styles.stopBtn} onClick={() => { void handleStop(); }} disabled={isLoading}>
              <Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}
            </button>
          )}
        </div>
      </div>

      {editorOpen && editingRule && !runtimeLocked && (
        <AutoReplyRuleEditor
          rule={editingRule}
          onSave={handleSaveRule}
          onCancel={() => { setEditorOpen(false); setEditingRule(null); }}
        />
      )}

      {renameOpen && !runtimeLocked && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={handleCancelRename}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={event => event.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.renameTitle")}</h3>
            <input
              className={`${styles.renameInput} liquid-glass-input`}
              type="text"
              value={renameValue}
              onChange={event => setRenameValue(event.target.value)}
              onKeyDown={event => {
                if (event.key === "Enter") handleConfirmRename();
                else if (event.key === "Escape") handleCancelRename();
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
        document.body,
      )}

      {importOpen && importData && !runtimeLocked && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={() => { setImportOpen(false); setImportData(null); }}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={event => event.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.importConfirmTitle")}</h3>
            <p className={styles.importInfo}>
              {t("sendBar.importName")}: {importData.name}<br />
              {t("sendBar.importRules")}: {importData.rules.length}
            </p>
            <div className={styles.renameBtns}>
              <button className={`${styles.renameCancelBtn} liquid-glass-button`} onClick={() => { setImportOpen(false); setImportData(null); }}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-glass-button`} onClick={() => { void handleImportAppend(); }}>
                {t("sendBar.importAppend")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-primary-button`} onClick={() => { void handleImportOverwrite(); }} disabled={!activeConfig}>
                {t("sendBar.importOverwrite")}
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )}

      <ConfirmDialog
        open={configDeleteConfirm && !runtimeLocked}
        title={t("sendBar.confirmDeleteHint")}
        message={activeConfig?.name}
        intent="danger"
        size="compact"
        onConfirm={handleDeleteConfig}
        onCancel={() => setConfigDeleteConfirm(false)}
      />
      <ConfirmDialog
        open={deleteConfirmId !== null && !runtimeLocked}
        title={t("commandPanel.confirmDelete")}
        message={deletingRule?.label?.trim() || (deletingRule?.triggerType === "timer" ? `${deletingRule.timerIntervalMs}ms` : deletingRule?.conditions[0]?.pattern)}
        intent="danger"
        size="compact"
        onConfirm={confirmDeleteRule}
        onCancel={() => setDeleteConfirmId(null)}
      />
    </div>
  );
}
