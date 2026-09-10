import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import ConfirmDialog from "../common/ConfirmDialog";
import Icon from "../common/Icon";
import LuaHelpModal from "./LuaHelpModal";
import { useSendBar } from "./SendBarContext";
import type { ScriptRecord } from "./types";
import { BUILTIN_SCRIPTS } from "./builtinScripts";
import { ASSET_KEYS, clearAsset, persistAsset } from "./assetStore";
import { parseScriptImport, uniqueAssetName, type ScriptImport } from "./assetValidation";
import styles from "./ScriptEditor.module.css";

interface ScriptEditorProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

function makeId(): string {
  return crypto.randomUUID();
}

function defaultScript(name: string): ScriptRecord {
  const now = Date.now();
  return {
    id: makeId(),
    name,
    code: `-- ${name}\n-- Write your Lua script here\n\non_data("ping", function(data)\n    log("Received: " .. data)\n    sleep(10)\n    send("pong\\r\\n")\nend)\n`,
    createdAt: now,
    updatedAt: now,
  };
}

export default function ScriptEditor({ sessionId, isActive, onRunningChange }: ScriptEditorProps) {
  const { t } = useTranslation();
  const { isSessionConnected } = useSession();
  const { showToast } = useToast();
  const isConnected = isSessionConnected(sessionId);

  const { state: sendBarState, dispatch } = useSendBar();
  const { scripts, activeScriptId, code, isRunning } = sendBarState.script;
  const scriptLogs = sendBarState.scriptLogs;

  const [outputExpanded, setOutputExpanded] = useState(false);
  const [scriptDeleteConfirm, setScriptDeleteConfirm] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const [importData, setImportData] = useState<ScriptImport | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const outputRef = useRef<HTMLDivElement>(null);

  const activeScript = scripts.find(script => script.id === activeScriptId);
  const isDirty = activeScript != null && code !== activeScript.code;

  // 会话断开时自动停止脚本引擎
  useEffect(() => {
    const unlisten = listen<{ session_id: string }>("session-disconnected", event => {
      if (event.payload.session_id === sessionId && isRunning) {
        dispatch({ type: "SET_SCRIPT_RUNNING", running: false });
        onRunningChange?.(false);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, isRunning, dispatch, onRunningChange]);

  useEffect(() => {
    if (outputRef.current) outputRef.current.scrollTop = outputRef.current.scrollHeight;
  }, [scriptLogs]);

  const persistScripts = useCallback((updated: ScriptRecord[]) => {
    dispatch({ type: "SET_SCRIPTS", scripts: updated });
    return persistAsset(ASSET_KEYS.scripts, updated);
  }, [dispatch]);

  // Active selection is a local session concern. The persisted key is only a default for newly
  // mounted SendBars; other mounted sessions no longer subscribe to it.
  const persistActive = useCallback((id: string | null) => {
    dispatch({ type: "SET_ACTIVE_SCRIPT", id });
    return id
      ? persistAsset(ASSET_KEYS.activeScriptId, id)
      : clearAsset(ASSET_KEYS.activeScriptId);
  }, [dispatch]);

  // ── 脚本管理 ──
  const handleSelectScript = useCallback((id: string) => {
    if (isRunning) return;
    void persistActive(id);
    const script = scripts.find(item => item.id === id);
    if (script) dispatch({ type: "SET_SCRIPT_CODE", code: script.code });
  }, [isRunning, scripts, dispatch, persistActive]);

  const handleNewScript = useCallback(() => {
    if (isRunning) return;
    const names = new Set(scripts.map(script => script.name));
    let index = scripts.length + 1;
    let name = t("sendBar.newScriptName", { n: index });
    while (names.has(name)) {
      index += 1;
      name = t("sendBar.newScriptName", { n: index });
    }

    const script = defaultScript(name);
    void persistScripts([...scripts, script]);
    void persistActive(script.id);
    dispatch({ type: "SET_SCRIPT_CODE", code: script.code });
  }, [isRunning, scripts, persistScripts, persistActive, dispatch, t]);

  const handleRenameScript = useCallback(() => {
    if (isRunning || !activeScriptId) return;
    setRenameValue(activeScript?.name || "");
    setRenameOpen(true);
  }, [isRunning, activeScriptId, activeScript]);

  const handleConfirmRename = useCallback(() => {
    if (isRunning) return;
    const newName = renameValue.trim();
    if (!newName || newName === activeScript?.name) {
      setRenameOpen(false);
      return;
    }
    if (scripts.some(script => script.id !== activeScriptId && script.name === newName)) {
      showToast("error", t("sendBar.nameExists", { defaultValue: "Name already exists" }));
      return;
    }

    const updated = scripts.map(script =>
      script.id === activeScriptId ? { ...script, name: newName, updatedAt: Date.now() } : script,
    );
    void persistScripts(updated);
    setRenameOpen(false);
  }, [isRunning, renameValue, activeScript, activeScriptId, scripts, persistScripts, showToast, t]);

  const handleCancelRename = useCallback(() => setRenameOpen(false), []);

  const handleDeleteScript = useCallback(() => {
    if (isRunning || !activeScriptId) return;
    const updated = scripts.filter(script => script.id !== activeScriptId);
    void persistScripts(updated);
    const next = updated[0];
    void persistActive(next?.id || null);
    dispatch({ type: "SET_SCRIPT_CODE", code: next?.code || "" });
    setScriptDeleteConfirm(false);
  }, [isRunning, activeScriptId, scripts, persistScripts, persistActive, dispatch]);

  useEffect(() => {
    setScriptDeleteConfirm(false);
  }, [activeScriptId]);

  // ── 代码编辑 ──
  const handleCodeChange = useCallback((newCode: string) => {
    if (!isRunning) dispatch({ type: "SET_SCRIPT_CODE", code: newCode });
  }, [isRunning, dispatch]);

  const handleSave = useCallback(async () => {
    if (isRunning || !activeScriptId) return false;
    const updated = scripts.map(script =>
      script.id === activeScriptId ? { ...script, code, updatedAt: Date.now() } : script,
    );
    return persistScripts(updated);
  }, [isRunning, activeScriptId, code, scripts, persistScripts]);

  // Ctrl+S — 仅在脚本面板活跃且未运行时生效
  useEffect(() => {
    if (!isActive || isRunning) return;
    const handler = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void handleSave();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [handleSave, isActive, isRunning]);

  // ── 执行控制 ──
  const handleStart = useCallback(async () => {
    if (isRunning || !isConnected || !code.trim()) return;
    await handleSave();
    try {
      // Runtime is an immutable snapshot. Editing controls are locked until stop/disconnect.
      await invoke("start_script_engine", { sessionId, code });
      dispatch({ type: "SET_SCRIPT_RUNNING", running: true });
      onRunningChange?.(true);
      setRenameOpen(false);
      setImportOpen(false);
      setImportData(null);
      setScriptDeleteConfirm(false);
    } catch (e) {
      dispatch({ type: "APPEND_SCRIPT_LOG", message: `[Error] ${e}` });
    }
  }, [isRunning, isConnected, code, sessionId, dispatch, onRunningChange, handleSave]);

  const handleStop = useCallback(async () => {
    try {
      await invoke("stop_script_engine", { sessionId });
      dispatch({ type: "SET_SCRIPT_RUNNING", running: false });
      onRunningChange?.(false);
    } catch (e) {
      dispatch({ type: "APPEND_SCRIPT_LOG", message: `[Error] ${e}` });
    }
  }, [sessionId, dispatch, onRunningChange]);

  // ── 导入/导出 ──
  const handleExportJSON = useCallback(() => {
    const script = activeScript;
    if (!script) return;
    const json = JSON.stringify(script, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(script.name || "script").replace(/\s+/g, "_")}.tauterm-script.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [activeScript]);

  const handleExportLua = useCallback(() => {
    const script = activeScript;
    if (!script) return;
    const blob = new Blob([script.code], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(script.name || "script").replace(/\s+/g, "_")}.lua`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [activeScript]);

  const handleLoadBuiltinExamples = useCallback(async () => {
    if (isRunning) return;
    const existingIds = new Set(scripts.map(script => script.id));
    const newBuiltins = BUILTIN_SCRIPTS.filter(script => !existingIds.has(script.id));
    if (newBuiltins.length === 0) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    if (await persistScripts([...scripts, ...newBuiltins])) {
      showToast("success", t("sendBar.builtinScriptsLoaded", { count: newBuiltins.length }));
    }
  }, [isRunning, scripts, persistScripts, showToast, t]);

  const handleImport = useCallback(() => {
    if (isRunning) return;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,.lua,.txt";
    input.onchange = async event => {
      const file = (event.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const isJSON = file.name.toLowerCase().endsWith(".json") || text.trim().startsWith("{");
        if (isJSON) {
          setImportData(parseScriptImport(JSON.parse(text)));
        } else {
          const name = file.name.replace(/\.(lua|txt)$/i, "").trim() || "Imported Script";
          setImportData({ name, code: text });
        }
        setImportOpen(true);
      } catch (err) {
        showToast("error", `${t("sendBar.importScriptFailed")}: ${String(err)}`);
      }
    };
    input.click();
  }, [isRunning, showToast, t]);

  const handleImportOverwrite = useCallback(async () => {
    if (isRunning || !importData || !activeScriptId) return;
    const name = uniqueAssetName(
      importData.name,
      scripts.filter(script => script.id !== activeScriptId).map(script => script.name),
      t("sendBar.imported"),
    );
    const updated = scripts.map(script =>
      script.id === activeScriptId
        ? { ...script, name, code: importData.code, updatedAt: Date.now() }
        : script,
    );
    const saved = await persistScripts(updated);
    dispatch({ type: "SET_SCRIPT_CODE", code: importData.code });
    setImportOpen(false);
    setImportData(null);
    if (saved) showToast("success", t("sendBar.importScriptSuccess"));
  }, [isRunning, importData, activeScriptId, scripts, persistScripts, dispatch, showToast, t]);

  const handleImportAppend = useCallback(async () => {
    if (isRunning || !importData) return;
    const now = Date.now();
    const newScript: ScriptRecord = {
      id: makeId(),
      name: uniqueAssetName(importData.name, scripts.map(script => script.name), t("sendBar.imported")),
      code: importData.code,
      createdAt: now,
      updatedAt: now,
    };
    const [savedScripts, savedActive] = await Promise.all([
      persistScripts([...scripts, newScript]),
      persistActive(newScript.id),
    ]);
    dispatch({ type: "SET_SCRIPT_CODE", code: newScript.code });
    setImportOpen(false);
    setImportData(null);
    if (savedScripts && savedActive) {
      showToast("success", t("sendBar.importScriptSuccess"));
    }
  }, [isRunning, importData, scripts, persistScripts, persistActive, dispatch, showToast, t]);

  const lineCount = code.split("\n").length;

  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <div className={styles.configActions}>
          <select
            className={`${styles.scriptSelect} liquid-glass-input liquid-glass-select`}
            value={activeScriptId || ""}
            onChange={event => handleSelectScript(event.target.value)}
            disabled={isRunning}
          >
            {scripts.map(script => (
              <option key={script.id} value={script.id}>{script.name}</option>
            ))}
          </select>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleNewScript} title={t("sendBar.new")} disabled={isRunning}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleRenameScript} title={t("sendBar.rename")} disabled={isRunning || !activeScriptId}>
            <Icon name="edit" size="sm" />
          </button>
          <button
            className={`${styles.toolBtn} liquid-glass-button`}
            onClick={() => setScriptDeleteConfirm(true)}
            disabled={isRunning || !activeScriptId}
            title={t("sendBar.delete")}
          >
            <Icon name="trash" size="sm" />
          </button>
        </div>
        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => setHelpOpen(true)} title={t("sendBar.helpTitle")}>
            {t("sendBar.help")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleLoadBuiltinExamples} title={t("sendBar.loadBuiltinScripts")} disabled={isRunning}>
            {t("sendBar.loadBuiltinScripts")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportJSON} disabled={!activeScriptId} title={t("sendBar.exportScriptJSON")}>
            {t("sendBar.exportScriptJSON")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportLua} disabled={!activeScriptId} title={t("sendBar.exportScriptLua")}>
            {t("sendBar.exportScriptLua")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleImport} title={t("sendBar.importScript")} disabled={isRunning}>
            {t("sendBar.importScript")}
          </button>
          <button className={`${styles.saveBtn} liquid-glass-button`} onClick={() => { void handleSave(); }} disabled={isRunning || !isDirty} title={t("sendBar.save")}>
            {t("sendBar.save")}
          </button>
        </div>
      </div>

      {scripts.length === 0 ? (
        <div className={styles.empty}>{t("sendBar.noScripts")}</div>
      ) : (
        <>
          <div className={styles.editorContainer}>
            <div className={styles.lineNumbers}>
              {Array.from({ length: Math.max(lineCount, 1) }, (_, index) => (
                <span key={index}>{index + 1}</span>
              ))}
            </div>
            <textarea
              ref={textareaRef}
              className={styles.editor}
              value={code}
              onChange={event => handleCodeChange(event.target.value)}
              placeholder={t("sendBar.scriptPlaceholder")}
              spellCheck={false}
              readOnly={isRunning}
              aria-readonly={isRunning}
            />
          </div>

          <div className={`${styles.output} ${outputExpanded ? "" : styles.outputCollapsed}`}>
            <div className={styles.outputHeader} onClick={() => setOutputExpanded(!outputExpanded)}>
              <span className={styles.outputTitle}>
                {t("sendBar.scriptOutput")}
                {scriptLogs.length > 0 && ` (${scriptLogs.length})`}
              </span>
              <div className={styles.outputActions}>
                {scriptLogs.length > 0 && (
                  <button
                    className={`${styles.outputClearBtn} liquid-glass-button`}
                    onClick={event => { event.stopPropagation(); dispatch({ type: "CLEAR_SCRIPT_LOGS" }); }}
                    title={t("sendBar.clearOutput")}
                  >
                    <Icon name="trash" size="xs" />
                  </button>
                )}
                <Icon name={outputExpanded ? "chevron-down" : "chevron-right"} size="sm" />
              </div>
            </div>
            {outputExpanded && (
              <div ref={outputRef} className={styles.outputContent}>
                {scriptLogs.length === 0 && <div className={styles.outputEmpty}>{t("sendBar.noOutput")}</div>}
                {scriptLogs.map((message, index) => (
                  <div key={index} className={styles.outputLine}>{message}</div>
                ))}
              </div>
            )}
          </div>
        </>
      )}

      <div className={styles.controls}>
        <span className={styles.status}>
          <Icon name={isRunning ? "status-connected" : "status-idle"} size={10} />
          {isRunning ? t("sendBar.running") : t("sendBar.stopped")}
        </span>
        <div className={styles.controlBtns}>
          {!isRunning ? (
            <button className={`${styles.startBtn} liquid-primary-button`} onClick={() => { void handleStart(); }} disabled={!isConnected || !code.trim()}>
              <Icon name="play" size="xs" /> {t("commandPanel.start")}
            </button>
          ) : (
            <button className={styles.stopBtn} onClick={() => { void handleStop(); }}>
              <Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}
            </button>
          )}
        </div>
      </div>

      {renameOpen && createPortal(
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

      <LuaHelpModal isOpen={helpOpen} onClose={() => setHelpOpen(false)} />

      {importOpen && importData && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={() => { setImportOpen(false); setImportData(null); }}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={event => event.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.importScriptConfirmTitle")}</h3>
            <p className={styles.importInfo}>
              {t("sendBar.importScriptName")}: {importData.name}<br />
              {t("sendBar.importScriptLines")}: {importData.code ? importData.code.split("\n").length : 0}
            </p>
            <div className={styles.renameBtns}>
              <button className={`${styles.renameCancelBtn} liquid-glass-button`} onClick={() => { setImportOpen(false); setImportData(null); }}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-glass-button`} onClick={() => { void handleImportAppend(); }}>
                {t("sendBar.importScriptAppend")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-primary-button`} onClick={() => { void handleImportOverwrite(); }} disabled={!activeScriptId}>
                {t("sendBar.importScriptOverwrite")}
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )}

      <ConfirmDialog
        open={scriptDeleteConfirm}
        title={t("sendBar.confirmDeleteScript")}
        message={activeScript?.name}
        intent="danger"
        size="compact"
        onConfirm={handleDeleteScript}
        onCancel={() => setScriptDeleteConfirm(false)}
      />
    </div>
  );
}
