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
  const [isTransitioning, setIsTransitioning] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const outputRef = useRef<HTMLDivElement>(null);
  const transitionAttemptRef = useRef(0);
  const runtimeLocked = isRunning || isTransitioning;

  const activeScript = scripts.find(script => script.id === activeScriptId);
  const isDirty = activeScript != null && code !== activeScript.code;

  useEffect(() => {
    return () => {
      transitionAttemptRef.current += 1;
    };
  }, []);

  // 会话断开时使任何启动尝试失效，并释放当前运行态。
  useEffect(() => {
    const unlisten = listen<{ session_id: string }>("session-disconnected", event => {
      if (event.payload.session_id !== sessionId) return;
      transitionAttemptRef.current += 1;
      if (isTransitioning) setIsTransitioning(false);
      if (isRunning) dispatch({ type: "SET_SCRIPT_RUNNING", running: false });
      if (isTransitioning || isRunning) onRunningChange?.(false);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, isRunning, isTransitioning, dispatch, onRunningChange]);

  useEffect(() => {
    if (outputRef.current) outputRef.current.scrollTop = outputRef.current.scrollHeight;
  }, [scriptLogs]);

  const persistScripts = useCallback((updated: ScriptRecord[]) => {
    dispatch({ type: "SET_SCRIPTS", scripts: updated });
    return persistAsset(ASSET_KEYS.scripts, updated);
  }, [dispatch]);

  // Active selection is session-local; this key is only the default for a newly mounted session.
  const persistActive = useCallback((id: string | null) => {
    dispatch({ type: "SET_ACTIVE_SCRIPT", id });
    return id
      ? persistAsset(ASSET_KEYS.activeScriptId, id)
      : clearAsset(ASSET_KEYS.activeScriptId);
  }, [dispatch]);

  // ── 脚本管理 ──
  const handleSelectScript = useCallback((id: string) => {
    if (runtimeLocked) return;
    void persistActive(id);
    const script = scripts.find(item => item.id === id);
    if (script) dispatch({ type: "SET_SCRIPT_CODE", code: script.code });
  }, [runtimeLocked, scripts, dispatch, persistActive]);

  const handleNewScript = useCallback(() => {
    if (runtimeLocked) return;
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
  }, [runtimeLocked, scripts, persistScripts, persistActive, dispatch, t]);

  const handleRenameScript = useCallback(() => {
    if (runtimeLocked || !activeScriptId) return;
    setRenameValue(activeScript?.name || "");
    setRenameOpen(true);
  }, [runtimeLocked, activeScriptId, activeScript]);

  const handleConfirmRename = useCallback(() => {
    if (runtimeLocked) return;
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
  }, [runtimeLocked, renameValue, activeScript, activeScriptId, scripts, persistScripts, showToast, t]);

  const handleCancelRename = useCallback(() => setRenameOpen(false), []);

  const handleDeleteScript = useCallback(() => {
    if (runtimeLocked || !activeScriptId) return;
    const updated = scripts.filter(script => script.id !== activeScriptId);
    void persistScripts(updated);
    const next = updated[0];
    void persistActive(next?.id || null);
    dispatch({ type: "SET_SCRIPT_CODE", code: next?.code || "" });
    setScriptDeleteConfirm(false);
  }, [runtimeLocked, activeScriptId, scripts, persistScripts, persistActive, dispatch]);

  useEffect(() => {
    setScriptDeleteConfirm(false);
  }, [activeScriptId]);

  // ── 代码编辑 ──
  const handleCodeChange = useCallback((newCode: string) => {
    if (!runtimeLocked) dispatch({ type: "SET_SCRIPT_CODE", code: newCode });
  }, [runtimeLocked, dispatch]);

  const handleSave = useCallback(async () => {
    if (runtimeLocked || !activeScriptId) return false;
    const updated = scripts.map(script =>
      script.id === activeScriptId ? { ...script, code, updatedAt: Date.now() } : script,
    );
    return persistScripts(updated);
  }, [runtimeLocked, activeScriptId, code, scripts, persistScripts]);

  useEffect(() => {
    if (!isActive || runtimeLocked) return;
    const handler = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void handleSave();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [handleSave, isActive, runtimeLocked]);

  // ── 执行控制 ──
  const handleStart = useCallback(async () => {
    if (runtimeLocked || !isConnected || !code.trim()) return;

    const attempt = transitionAttemptRef.current + 1;
    transitionAttemptRef.current = attempt;
    setIsTransitioning(true);
    onRunningChange?.(true);
    setRenameOpen(false);
    setImportOpen(false);
    setImportData(null);
    setScriptDeleteConfirm(false);

    let started = false;
    try {
      // Save the exact runtime snapshot without calling handleSave: the transition lock is already
      // active and must stay active across this first asynchronous persistence boundary.
      if (activeScriptId) {
        const updated = scripts.map(script =>
          script.id === activeScriptId ? { ...script, code, updatedAt: Date.now() } : script,
        );
        await persistScripts(updated);
      }
      if (transitionAttemptRef.current !== attempt) return;

      await invoke("start_script_engine", { sessionId, code });
      if (transitionAttemptRef.current !== attempt) {
        void invoke("stop_script_engine", { sessionId }).catch(() => undefined);
        return;
      }

      dispatch({ type: "SET_SCRIPT_RUNNING", running: true });
      started = true;
    } catch (error) {
      if (transitionAttemptRef.current === attempt) {
        dispatch({ type: "APPEND_SCRIPT_LOG", message: `[Error] ${String(error)}` });
      }
    } finally {
      if (transitionAttemptRef.current === attempt) {
        setIsTransitioning(false);
        if (!started) onRunningChange?.(false);
      }
    }
  }, [runtimeLocked, isConnected, code, activeScriptId, scripts, persistScripts, sessionId, dispatch, onRunningChange]);

  const handleStop = useCallback(async () => {
    if (!isRunning || isTransitioning) return;
    setIsTransitioning(true);
    try {
      await invoke("stop_script_engine", { sessionId });
      dispatch({ type: "SET_SCRIPT_RUNNING", running: false });
      setIsTransitioning(false);
      onRunningChange?.(false);
    } catch (error) {
      setIsTransitioning(false);
      dispatch({ type: "APPEND_SCRIPT_LOG", message: `[Error] ${String(error)}` });
    }
  }, [sessionId, isRunning, isTransitioning, dispatch, onRunningChange]);

  // ── 导入/导出 ──
  const handleExportJSON = useCallback(() => {
    if (runtimeLocked) return;
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
  }, [runtimeLocked, activeScript]);

  const handleExportLua = useCallback(() => {
    if (runtimeLocked) return;
    const script = activeScript;
    if (!script) return;
    const blob = new Blob([script.code], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${(script.name || "script").replace(/\s+/g, "_")}.lua`;
    anchor.click();
    URL.revokeObjectURL(url);
  }, [runtimeLocked, activeScript]);

  const handleLoadBuiltinExamples = useCallback(async () => {
    if (runtimeLocked) return;
    const existingIds = new Set(scripts.map(script => script.id));
    const newBuiltins = BUILTIN_SCRIPTS.filter(script => !existingIds.has(script.id));
    if (newBuiltins.length === 0) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    if (await persistScripts([...scripts, ...newBuiltins])) {
      showToast("success", t("sendBar.builtinScriptsLoaded", { count: newBuiltins.length }));
    }
  }, [runtimeLocked, scripts, persistScripts, showToast, t]);

  const handleImport = useCallback(() => {
    if (runtimeLocked) return;
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
      } catch (error) {
        showToast("error", `${t("sendBar.importScriptFailed")}: ${String(error)}`);
      }
    };
    input.click();
  }, [runtimeLocked, showToast, t]);

  const handleImportOverwrite = useCallback(async () => {
    if (runtimeLocked || !importData || !activeScriptId) return;
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
  }, [runtimeLocked, importData, activeScriptId, scripts, persistScripts, dispatch, showToast, t]);

  const handleImportAppend = useCallback(async () => {
    if (runtimeLocked || !importData) return;
    const now = Date.now();
    const newScript: ScriptRecord = {
      id: makeId(),
      name: uniqueAssetName(importData.name, scripts.map(script => script.name), t("sendBar.imported")),
      code: importData.code,
      createdAt: now,
      updatedAt: now,
    };
    const savedScripts = await persistScripts([...scripts, newScript]);
    if (!savedScripts) return;
    const savedActive = await persistActive(newScript.id);
    dispatch({ type: "SET_SCRIPT_CODE", code: newScript.code });
    setImportOpen(false);
    setImportData(null);
    if (savedActive) showToast("success", t("sendBar.importScriptSuccess"));
  }, [runtimeLocked, importData, scripts, persistScripts, persistActive, dispatch, showToast, t]);

  const lineCount = code.split("\n").length;

  return (
    <div className={styles.panel} aria-busy={isTransitioning || undefined}>
      <div className={styles.toolbar}>
        <div className={styles.configActions}>
          <select
            className={`${styles.scriptSelect} liquid-glass-input liquid-glass-select`}
            value={activeScriptId || ""}
            onChange={event => handleSelectScript(event.target.value)}
            disabled={runtimeLocked}
          >
            {scripts.map(script => (
              <option key={script.id} value={script.id}>{script.name}</option>
            ))}
          </select>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleNewScript} title={t("sendBar.new")} disabled={runtimeLocked}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleRenameScript} title={t("sendBar.rename")} disabled={runtimeLocked || !activeScriptId}>
            <Icon name="edit" size="sm" />
          </button>
          <button
            className={`${styles.toolBtn} liquid-glass-button`}
            onClick={() => setScriptDeleteConfirm(true)}
            disabled={runtimeLocked || !activeScriptId}
            title={t("sendBar.delete")}
          >
            <Icon name="trash" size="sm" />
          </button>
        </div>
        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => setHelpOpen(true)} title={t("sendBar.helpTitle")}>
            {t("sendBar.help")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleLoadBuiltinExamples(); }} title={t("sendBar.loadBuiltinScripts")} disabled={runtimeLocked}>
            {t("sendBar.loadBuiltinScripts")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportJSON} disabled={runtimeLocked || !activeScriptId} title={t("sendBar.exportScriptJSON")}>
            {t("sendBar.exportScriptJSON")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportLua} disabled={runtimeLocked || !activeScriptId} title={t("sendBar.exportScriptLua")}>
            {t("sendBar.exportScriptLua")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleImport} title={t("sendBar.importScript")} disabled={runtimeLocked}>
            {t("sendBar.importScript")}
          </button>
          <button className={`${styles.saveBtn} liquid-glass-button`} onClick={() => { void handleSave(); }} disabled={runtimeLocked || !isDirty} title={t("sendBar.save")}>
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
              readOnly={runtimeLocked}
              aria-readonly={runtimeLocked}
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
            <button className={`${styles.startBtn} liquid-primary-button`} onClick={() => { void handleStart(); }} disabled={runtimeLocked || !isConnected || !code.trim()}>
              <Icon name="play" size="xs" /> {t("commandPanel.start")}
            </button>
          ) : (
            <button className={styles.stopBtn} onClick={() => { void handleStop(); }} disabled={isTransitioning}>
              <Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}
            </button>
          )}
        </div>
      </div>

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

      <LuaHelpModal isOpen={helpOpen} onClose={() => setHelpOpen(false)} />

      {importOpen && importData && !runtimeLocked && createPortal(
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
        open={scriptDeleteConfirm && !runtimeLocked}
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
