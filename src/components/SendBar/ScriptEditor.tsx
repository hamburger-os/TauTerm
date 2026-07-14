import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { useSendBar } from "./SendBarContext";
import Icon from "../common/Icon";
import LuaHelpModal from "./LuaHelpModal";
import type { ScriptRecord } from "./types";
import { BUILTIN_SCRIPTS } from "./builtinScripts";
import styles from "./ScriptEditor.module.css";

interface ScriptEditorProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

const STORAGE_KEY_SCRIPTS = "tauterm-scripts";
const STORAGE_KEY_ACTIVE = "tauterm-active-script-id";

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
  const { state: sessionState } = useSession();
  const activeTab = sessionState.tabs.find(tab => tab.id === sessionId);
  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";

  const { state: sendBarState, dispatch } = useSendBar();
  const { scripts, activeScriptId, code, isRunning } = sendBarState.script;
  const scriptLogs = sendBarState.scriptLogs;
  const { showToast } = useToast();

  const [outputExpanded, setOutputExpanded] = useState(false);
  const [scriptDeleteConfirm, setScriptDeleteConfirm] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const [importData, setImportData] = useState<ScriptRecord | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const outputRef = useRef<HTMLDivElement>(null);

  // 当前活跃脚本
  const activeScript = scripts.find(s => s.id === activeScriptId);

  // 未保存修改指示
  const isDirty = activeScript != null && code !== activeScript.code;

  // 会话断开时自动停止脚本引擎
  useEffect(() => {
    const unlisten = listen<{ session_id: string }>("session-disconnected", (event) => {
      if (event.payload.session_id === sessionId && isRunning) {
        dispatch({ type: "SET_SCRIPT_RUNNING", running: false });
        onRunningChange?.(false);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, [sessionId, isRunning, dispatch, onRunningChange]);

  // 自动滚动到底部
  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [scriptLogs]);

  // 持久化
  const persistScripts = useCallback((updated: ScriptRecord[]) => {
    dispatch({ type: "SET_SCRIPTS", scripts: updated });
    localStorage.setItem(STORAGE_KEY_SCRIPTS, JSON.stringify(updated));
  }, [dispatch]);

  const persistActive = useCallback((id: string | null) => {
    dispatch({ type: "SET_ACTIVE_SCRIPT", id });
    if (id) localStorage.setItem(STORAGE_KEY_ACTIVE, id);
    else localStorage.removeItem(STORAGE_KEY_ACTIVE);
  }, [dispatch]);

  // ── 脚本管理 ──
  const handleSelectScript = useCallback((id: string) => {
    persistActive(id);
    const script = scripts.find(s => s.id === id);
    if (script) {
      dispatch({ type: "SET_SCRIPT_CODE", code: script.code });
    }
  }, [scripts, dispatch, persistActive]);

  const handleNewScript = useCallback(() => {
    const name = t("sendBar.newScriptName", { n: scripts.length + 1 });
    const script = defaultScript(name);
    const updated = [...scripts, script];
    persistScripts(updated);
    persistActive(script.id);
    dispatch({ type: "SET_SCRIPT_CODE", code: script.code });
  }, [scripts, persistScripts, persistActive, dispatch]);

  const handleRenameScript = useCallback(() => {
    if (!activeScriptId) return;
    const currentName = activeScript?.name || "";
    setRenameValue(currentName);
    setRenameOpen(true);
  }, [activeScriptId, activeScript]);

  const handleConfirmRename = useCallback(() => {
    const newName = renameValue.trim();
    if (!newName || newName === activeScript?.name) {
      setRenameOpen(false);
      return;
    }
    const updated = scripts.map(s =>
      s.id === activeScriptId ? { ...s, name: newName, updatedAt: Date.now() } : s
    );
    persistScripts(updated);
    setRenameOpen(false);
  }, [renameValue, activeScript, activeScriptId, scripts, persistScripts]);

  const handleCancelRename = useCallback(() => {
    setRenameOpen(false);
  }, []);

  const handleDeleteScript = useCallback(() => {
    if (!activeScriptId) return;
    if (!scriptDeleteConfirm) {
      setScriptDeleteConfirm(true);
      return;
    }
    // 确认删除
    const updated = scripts.filter(s => s.id !== activeScriptId);
    persistScripts(updated);
    const next = updated[0];
    persistActive(next?.id || null);
    dispatch({ type: "SET_SCRIPT_CODE", code: next?.code || "" });
    setScriptDeleteConfirm(false);
  }, [activeScriptId, scriptDeleteConfirm, scripts, persistScripts, persistActive, dispatch]);

  // 切换活跃脚本时重置删除确认
  useEffect(() => {
    setScriptDeleteConfirm(false);
  }, [activeScriptId]);

  // ── 代码编辑 ──
  const handleCodeChange = useCallback((newCode: string) => {
    dispatch({ type: "SET_SCRIPT_CODE", code: newCode });
  }, [dispatch]);

  const handleSave = useCallback(() => {
    if (!activeScriptId) return;
    const updated = scripts.map(s =>
      s.id === activeScriptId ? { ...s, code, updatedAt: Date.now() } : s
    );
    persistScripts(updated);
  }, [activeScriptId, code, scripts, persistScripts]);

  // Ctrl+S — 仅在脚本面板活跃时生效，避免其它模式下误触发脚本保存
  useEffect(() => {
    if (!isActive) return;
    const handler = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key === "s") {
        e.preventDefault();
        handleSave();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [handleSave, isActive]);

  // ── 执行控制 ──
  const handleStart = useCallback(async () => {
    if (!isConnected || !code.trim()) return;
    handleSave();
    try {
      await invoke("start_script_engine", { sessionId, code });
      dispatch({ type: "SET_SCRIPT_RUNNING", running: true });
      onRunningChange?.(true);
    } catch (e) {
      dispatch({ type: "APPEND_SCRIPT_LOG", message: `[Error] ${e}` });
    }
  }, [isConnected, code, sessionId, dispatch, onRunningChange, handleSave]);

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
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(script.name || "script").replace(/\s+/g, "_")}.tauterm-script.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, [activeScript]);

  const handleExportLua = useCallback(() => {
    const script = activeScript;
    if (!script) return;
    const blob = new Blob([script.code], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${(script.name || "script").replace(/\s+/g, "_")}.lua`;
    a.click();
    URL.revokeObjectURL(url);
  }, [activeScript]);

  const handleLoadBuiltinExamples = useCallback(() => {
    const existingIds = new Set(scripts.map(s => s.id));
    const newBuiltins = BUILTIN_SCRIPTS.filter(s => !existingIds.has(s.id));
    if (newBuiltins.length === 0) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    persistScripts([...scripts, ...newBuiltins]);
    showToast("success", t("sendBar.builtinScriptsLoaded", { count: newBuiltins.length }));
  }, [scripts, persistScripts, showToast, t]);

  const handleImport = useCallback(() => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,.lua,.txt";
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const isJSON = file.name.endsWith(".json") || text.trim().startsWith("{");
        if (isJSON) {
          const parsed = JSON.parse(text);
          if (typeof parsed.name !== "string" || typeof parsed.code !== "string") {
            throw new Error("Invalid script format: missing name or code");
          }
          setImportData(parsed as ScriptRecord);
        } else {
          // Plain Lua code: use filename (without extension) as script name
          const name = file.name.replace(/\.(lua|txt)$/i, "");
          setImportData({
            id: "",
            name: name || "Imported Script",
            code: text,
            createdAt: 0,
            updatedAt: 0,
          });
        }
        setImportOpen(true);
      } catch (err) {
        showToast("error", `${t("sendBar.importScriptFailed")}: ${err}`);
      }
    };
    input.click();
  }, [showToast, t]);

  const handleImportOverwrite = useCallback(() => {
    if (!importData || !activeScriptId) return;
    const updated = scripts.map(s =>
      s.id === activeScriptId
        ? { ...s, name: importData.name, code: importData.code, updatedAt: Date.now() }
        : s
    );
    persistScripts(updated);
    dispatch({ type: "SET_SCRIPT_CODE", code: importData.code });
    setImportOpen(false);
    setImportData(null);
    showToast("success", t("sendBar.importScriptSuccess"));
  }, [importData, activeScriptId, scripts, persistScripts, dispatch, showToast, t]);

  const handleImportAppend = useCallback(() => {
    if (!importData) return;
    const now = Date.now();
    const newScript: ScriptRecord = {
      id: makeId(),
      name: `${importData.name || "Imported"} (${t("sendBar.imported")})`,
      code: importData.code,
      createdAt: now,
      updatedAt: now,
    };
    const updated = [...scripts, newScript];
    persistScripts(updated);
    persistActive(newScript.id);
    dispatch({ type: "SET_SCRIPT_CODE", code: newScript.code });
    setImportOpen(false);
    setImportData(null);
    showToast("success", t("sendBar.importScriptSuccess"));
  }, [importData, scripts, persistScripts, persistActive, dispatch, showToast, t]);

  // 行号计算
  const lineCount = code.split("\n").length;

  return (
    <div className={styles.panel}>
      {/* 脚本管理工具栏 */}
      <div className={styles.toolbar}>
        <div className={styles.configActions}>
          <select
            className={`${styles.scriptSelect} liquid-glass-input liquid-glass-select`}
            value={activeScriptId || ""}
            onChange={e => handleSelectScript(e.target.value)}
          >
            {scripts.map(s => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </select>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleNewScript} title={t("sendBar.new")}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleRenameScript} title={t("sendBar.rename")}>
            <Icon name="edit" size="sm" />
          </button>
          {scriptDeleteConfirm ? (
            <div className={styles.toolConfirm}>
              <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleDeleteScript}
                title={t("sendBar.confirmDeleteScript")}>
                <Icon name="warning" size="sm" />
                <span className={styles.deleteHint}>{t("sendBar.confirmDeleteHint")}</span>
              </button>
              <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => setScriptDeleteConfirm(false)}
                title={t("sendBar.cancel")}>
                <Icon name="close" size="sm" />
              </button>
            </div>
          ) : (
            <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleDeleteScript} disabled={!activeScriptId}
              title={t("sendBar.delete")}>
              <Icon name="trash" size="sm" />
            </button>
          )}
        </div>
        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`}
            onClick={() => setHelpOpen(true)}
            title={t("sendBar.helpTitle")}>
            {t("sendBar.help")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`}
            onClick={handleLoadBuiltinExamples}
            title={t("sendBar.loadBuiltinScripts")}>
            {t("sendBar.loadBuiltinScripts")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportJSON}
            disabled={!activeScriptId} title={t("sendBar.exportScriptJSON")}>
            {t("sendBar.exportScriptJSON")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleExportLua}
            disabled={!activeScriptId} title={t("sendBar.exportScriptLua")}>
            {t("sendBar.exportScriptLua")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleImport}
            title={t("sendBar.importScript")}>
            {t("sendBar.importScript")}
          </button>
          <button className={`${styles.saveBtn} liquid-glass-button`} onClick={handleSave}
            disabled={!isDirty} title={t("sendBar.save")}>
            {t("sendBar.save")}
          </button>
        </div>
      </div>

      {/* 代码编辑器 — 无脚本时显示空状态提示 */}
      {scripts.length === 0 ? (
        <div className={styles.empty}>
          {t("sendBar.noScripts")}
        </div>
      ) : (
      <>
      <div className={styles.editorContainer}>
        <div className={styles.lineNumbers}>
          {Array.from({ length: Math.max(lineCount, 1) }, (_, i) => (
            <span key={i}>{i + 1}</span>
          ))}
        </div>
        <textarea
          ref={textareaRef}
          className={styles.editor}
          value={code}
          onChange={e => handleCodeChange(e.target.value)}
          placeholder={t("sendBar.scriptPlaceholder")}
          spellCheck={false}
        />
      </div>

      {/* Script Output 面板 */}
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
                onClick={(e) => { e.stopPropagation(); dispatch({ type: "CLEAR_SCRIPT_LOGS" }); }}
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
            {scriptLogs.length === 0 && (
              <div className={styles.outputEmpty}>{t("sendBar.noOutput")}</div>
            )}
            {scriptLogs.map((msg, i) => (
              <div key={i} className={styles.outputLine}>{msg}</div>
            ))}
          </div>
        )}
      </div>
      </>
      )}

      {/* 执行控制 */}
      <div className={styles.controls}>
        <span className={styles.status}>
          <Icon name={isRunning ? "status-connected" : "status-idle"} size={10} />
          {isRunning ? t("sendBar.running") : t("sendBar.stopped")}
        </span>
        <div className={styles.controlBtns}>
          {!isRunning ? (
            <button className={`${styles.startBtn} liquid-primary-button`} onClick={handleStart} disabled={!isConnected || !code.trim()}>
              <Icon name="play" size="xs" /> {t("commandPanel.start")}
            </button>
          ) : (
            <button className={styles.stopBtn} onClick={handleStop}>
              <Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}
            </button>
          )}
        </div>
      </div>

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

      {/* 帮助弹窗 */}
      <LuaHelpModal isOpen={helpOpen} onClose={() => setHelpOpen(false)} />

      {/* 导入确认弹窗 */}
      {importOpen && importData && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={() => { setImportOpen(false); setImportData(null); }}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={e => e.stopPropagation()}>
            <h3 className={styles.renameTitle}>{t("sendBar.importScriptConfirmTitle")}</h3>
            <p className={styles.importInfo}>
              {t("sendBar.importScriptName")}: {importData.name}<br/>
              {t("sendBar.importScriptLines")}: {importData.code ? importData.code.split("\n").length : 0}
            </p>
            <div className={styles.renameBtns}>
              <button className={`${styles.renameCancelBtn} liquid-glass-button`}
                onClick={() => { setImportOpen(false); setImportData(null); }}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-glass-button`} onClick={handleImportAppend}>
                {t("sendBar.importScriptAppend")}
              </button>
              <button className={`${styles.renameSaveBtn} liquid-primary-button`} onClick={handleImportOverwrite}
                disabled={!activeScriptId}>
                {t("sendBar.importScriptOverwrite")}
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}
    </div>
  );
}
