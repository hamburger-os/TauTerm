import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useSession } from "../../context/SessionContext";
import { useToast } from "../../context/ToastContext";
import { usePointerDragReorder } from "../../hooks/usePointerDragReorder";
import ConfirmDialog from "../common/ConfirmDialog";
import Icon from "../common/Icon";
import { useSendBar } from "./SendBarContext";
import CommandEditorModal from "./CommandEditorModal";
import useCommandRunner from "./useCommandRunner";
import defaultCommands from "./default-commands.json";
import {
  ASSET_KEYS,
  clearAsset,
  loadAsset,
  persistAsset,
  subscribeAsset,
} from "./assetStore";
import { parseCommandConfig, uniqueAssetName } from "./assetValidation";
import type { CommandItem, CommandConfig } from "./types";
import styles from "./CommandPanel.module.css";

interface CommandPanelProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

const CONFIG_STORE_KEY = ASSET_KEYS.commandSets;
const ACTIVE_CONFIG_STORE_KEY = ASSET_KEYS.activeCommandSet;

function saveConfigs(configs: CommandConfig[]) {
  return persistAsset(CONFIG_STORE_KEY, configs);
}

function saveActiveConfig(name: string) {
  return name
    ? persistAsset(ACTIVE_CONFIG_STORE_KEY, name)
    : clearAsset(ACTIVE_CONFIG_STORE_KEY);
}

export default function CommandPanel({ sessionId, isActive, onRunningChange }: CommandPanelProps) {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const { sendToTarget, isSessionConnected } = useSession();
  const isConnected = isSessionConnected(sessionId);
  const { state: sendBarState, dispatch } = useSendBar();
  const { activeConfigName, selectedIds, loopCount } = sendBarState.command;

  const [configs, setConfigs] = useState<CommandConfig[]>([defaultCommands as CommandConfig]);
  const activeConfigNameRef = useRef(activeConfigName);
  activeConfigNameRef.current = activeConfigName;

  const setActiveConfigName = useCallback((name: string) => {
    dispatch({ type: "SET_ACTIVE_COMMAND_CONFIG", name });
  }, [dispatch]);

  const [editorOpen, setEditorOpen] = useState(false);
  const [editingItem, setEditingItem] = useState<CommandItem | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [configDeleteConfirm, setConfigDeleteConfirm] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  // Command Set 是全局工程资产；当前命令集选择由 SendBarContext 按会话保留。
  // 内置命令只在存储尚未初始化时播种一次；空数组代表用户明确删除了全部命令集。
  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      loadAsset<CommandConfig[]>(CONFIG_STORE_KEY),
      loadAsset<string>(ACTIVE_CONFIG_STORE_KEY),
    ]).then(([storedConfigs, storedActive]) => {
      if (cancelled) return;
      const hasStoredConfigs = Array.isArray(storedConfigs);
      const nextConfigs = hasStoredConfigs ? storedConfigs : [defaultCommands as CommandConfig];
      const sessionActive = activeConfigNameRef.current;
      const nextActive = sessionActive && nextConfigs.some(config => config.name === sessionActive)
        ? sessionActive
        : storedActive && nextConfigs.some(config => config.name === storedActive)
          ? storedActive
          : nextConfigs[0]?.name ?? "";

      setConfigs(nextConfigs);
      setActiveConfigName(nextActive);
      if (!hasStoredConfigs) void saveConfigs(nextConfigs);
    }).catch(() => {
      // 内置命令集保持可用；统一持久化层负责报告存储错误。
    });
    return () => { cancelled = true; };
  }, [setActiveConfigName]);

  useEffect(() => {
    return subscribeAsset<CommandConfig[]>(CONFIG_STORE_KEY, value => {
      const next = Array.isArray(value) ? value : [];
      setConfigs(next);

      const current = activeConfigNameRef.current;
      if (!next.some(config => config.name === current)) {
        setActiveConfigName(next[0]?.name ?? "");
        dispatch({ type: "CLEAR_COMMAND_SELECTION" });
      }
    });
  }, [dispatch, setActiveConfigName]);

  const activeConfig = useMemo(
    () => configs.find(config => config.name === activeConfigName) ?? configs[0],
    [configs, activeConfigName],
  );
  const commands = activeConfig?.commands ?? [];
  const defaultDelay = activeConfig?.defaultDelay ?? 500;
  const deletingCommand = useMemo(
    () => commands.find(command => command.id === deleteConfirmId) ?? null,
    [commands, deleteConfirmId],
  );

  const runner = useCommandRunner({
    onSend: useCallback(async (command: CommandItem) => {
      if (!isConnected) throw new Error(t("sendBar.disconnected"));
      try {
        await sendToTarget(sessionId, command.command + "\r\n");
      } catch (error) {
        showToast("error", String(error));
        throw error;
      }
    }, [sessionId, sendToTarget, isConnected, showToast, t]),
  });

  const onRunningChangeRef = useRef(onRunningChange);
  onRunningChangeRef.current = onRunningChange;
  useEffect(() => {
    if (runner.isRunning && isActive) {
      onRunningChangeRef.current?.(true);
      return () => onRunningChangeRef.current?.(false);
    }
  }, [runner.isRunning, isActive]);

  const persistConfig = useCallback((nextCommands: CommandItem[], delay = defaultDelay) => {
    setConfigs(previous => {
      const updated = previous.map(config =>
        config.name === activeConfigName
          ? { ...config, commands: nextCommands, defaultDelay: delay }
          : config,
      );
      void saveConfigs(updated);
      return updated;
    });
  }, [activeConfigName, defaultDelay]);

  const handleReorder = useCallback((next: CommandItem[]) => {
    persistConfig(next);
  }, [persistConfig]);

  const {
    isDragging,
    dropIndex,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
  } = usePointerDragReorder(commands, handleReorder, {
    itemSelector: `.${styles.commandRow}`,
    draggingClass: styles.rowDragging,
    disabled: runner.isRunning,
    listRef,
  });

  const handleConfigChange = useCallback((name: string) => {
    if (runner.isRunning) return;
    setActiveConfigName(name);
    void saveActiveConfig(name);
    dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    setDeleteConfirmId(null);
    setConfigDeleteConfirm(false);
  }, [runner.isRunning, dispatch, setActiveConfigName]);

  const handleRenameStart = useCallback(() => {
    if (runner.isRunning || !activeConfig) return;
    setRenameValue(activeConfig.name);
    setRenameOpen(true);
  }, [runner.isRunning, activeConfig]);

  const handleRenameConfirm = useCallback(() => {
    if (runner.isRunning) return;
    const newName = renameValue.trim();
    if (!newName || newName === activeConfigName) {
      setRenameOpen(false);
      return;
    }
    if (configs.some(config => config.name === newName)) {
      showToast("error", t("sendBar.nameExists", { defaultValue: "Name already exists" }));
      return;
    }

    setConfigs(previous => {
      const updated = previous.map(config =>
        config.name === activeConfigName ? { ...config, name: newName } : config,
      );
      void saveConfigs(updated);
      return updated;
    });
    setActiveConfigName(newName);
    void saveActiveConfig(newName);
    setRenameOpen(false);
  }, [runner.isRunning, renameValue, activeConfigName, configs, showToast, t, setActiveConfigName]);

  const handleRenameCancel = useCallback(() => setRenameOpen(false), []);

  const handleDeleteConfig = useCallback(() => {
    if (runner.isRunning || !activeConfig) return;
    const remaining = configs.filter(config => config.name !== activeConfigName);
    setConfigs(remaining);
    void saveConfigs(remaining);

    const nextActive = remaining[0]?.name ?? "";
    setActiveConfigName(nextActive);
    void saveActiveConfig(nextActive);
    dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    setDeleteConfirmId(null);
    setConfigDeleteConfirm(false);
  }, [runner.isRunning, activeConfig, configs, activeConfigName, dispatch, setActiveConfigName]);

  const handleAddConfig = useCallback(() => {
    if (runner.isRunning) return;
    const existing = new Set(configs.map(config => config.name));
    let counter = 1;
    let newName = t("commandPanel.newConfigName", { n: counter });
    while (existing.has(newName)) {
      counter += 1;
      newName = t("commandPanel.newConfigName", { n: counter });
    }
    const newConfig: CommandConfig = {
      version: 1,
      name: newName,
      defaultDelay: 500,
      commands: [],
    };
    const updated = [...configs, newConfig];
    setConfigs(updated);
    void saveConfigs(updated);
    setActiveConfigName(newName);
    void saveActiveConfig(newName);
    dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    setConfigDeleteConfirm(false);
  }, [runner.isRunning, configs, dispatch, t, setActiveConfigName]);

  const handleAdd = useCallback(() => {
    if (runner.isRunning) return;
    setEditingItem(null);
    setEditorOpen(true);
  }, [runner.isRunning]);

  const handleEdit = useCallback((item: CommandItem) => {
    if (runner.isRunning) return;
    setEditingItem(item);
    setEditorOpen(true);
  }, [runner.isRunning]);

  const handleDeleteConfirmed = useCallback(() => {
    if (runner.isRunning || !deleteConfirmId) return;
    persistConfig(commands.filter(command => command.id !== deleteConfirmId));
    if (selectedIds.has(deleteConfirmId)) {
      dispatch({ type: "TOGGLE_COMMAND_SELECT", id: deleteConfirmId });
    }
    setDeleteConfirmId(null);
  }, [runner.isRunning, deleteConfirmId, commands, persistConfig, selectedIds, dispatch]);

  const handleSaveCommand = useCallback((item: CommandItem) => {
    if (runner.isRunning) return;
    const index = commands.findIndex(command => command.id === item.id);
    if (index >= 0) {
      const next = [...commands];
      next[index] = item;
      persistConfig(next);
    } else {
      persistConfig([...commands, item]);
    }
  }, [runner.isRunning, commands, persistConfig]);

  const toggleSelect = useCallback((id: string) => {
    if (!runner.isRunning) dispatch({ type: "TOGGLE_COMMAND_SELECT", id });
  }, [runner.isRunning, dispatch]);

  const handleLoopCountChange = useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
    if (runner.isRunning) return;
    const value = Number(event.target.value);
    if (!Number.isFinite(value)) return;
    dispatch({ type: "SET_LOOP_COUNT", count: Math.max(0, Math.floor(value)) });
  }, [runner.isRunning, dispatch]);

  const handleDelayChange = useCallback((id: string, newDelay: number) => {
    if (runner.isRunning || !Number.isFinite(newDelay)) return;
    persistConfig(commands.map(command =>
      command.id === id ? { ...command, delay: Math.max(0, newDelay) } : command,
    ));
  }, [runner.isRunning, commands, persistConfig]);

  const handleStart = useCallback(() => {
    if (runner.isRunning) {
      runner.stop();
      return;
    }
    const selected = commands.filter(command => selectedIds.has(command.id));
    if (!isConnected || selected.length === 0) return;
    runner.start(selected, loopCount);
    setEditorOpen(false);
    setRenameOpen(false);
    setDeleteConfirmId(null);
    setConfigDeleteConfirm(false);
  }, [runner, commands, selectedIds, isConnected, loopCount]);

  useEffect(() => {
    if ((!isConnected || !isActive) && runner.isRunning) runner.stop();
  }, [isConnected, isActive, runner]);

  const handleSelectAll = useCallback(() => {
    if (runner.isRunning) return;
    if (selectedIds.size === commands.length) {
      dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    } else {
      dispatch({ type: "SELECT_ALL_COMMANDS", ids: commands.map(command => command.id) });
    }
  }, [runner.isRunning, commands, selectedIds, dispatch]);

  const handleImport = useCallback(async () => {
    if (runner.isRunning) return;
    try {
      const content = await invoke<string | null>("import_command_set_file");
      if (!content) return;
      const imported = parseCommandConfig(JSON.parse(content));
      const importName = uniqueAssetName(
        imported.name,
        configs.map(config => config.name),
        t("sendBar.imported"),
      );
      const newConfig = { ...imported, name: importName };
      const updated = [...configs, newConfig];
      setConfigs(updated);
      void saveConfigs(updated);
      setActiveConfigName(importName);
      void saveActiveConfig(importName);
      dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    } catch (error) {
      console.error("Import command set failed:", error);
      showToast("error", t("commandPanel.importFailed"));
    }
  }, [runner.isRunning, configs, showToast, t, dispatch, setActiveConfigName]);

  const handleLoadExamples = useCallback(async () => {
    if (runner.isRunning) return;
    const existingNames = new Set(configs.map(config => config.name));
    if (existingNames.has(defaultCommands.name)) {
      showToast("info", t("sendBar.noNewExamples"));
      return;
    }
    const newConfig = { ...defaultCommands, name: defaultCommands.name } as CommandConfig;
    const updated = [...configs, newConfig];
    setConfigs(updated);
    setActiveConfigName(defaultCommands.name);
    dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    const [savedConfigs, savedActive] = await Promise.all([
      saveConfigs(updated),
      saveActiveConfig(defaultCommands.name),
    ]);
    if (savedConfigs && savedActive) {
      showToast("success", t("sendBar.examplesLoaded", { count: 1 }));
    }
  }, [runner.isRunning, configs, showToast, t, dispatch, setActiveConfigName]);

  const handleExport = useCallback(async () => {
    if (runner.isRunning || !activeConfig) return;
    try {
      await invoke<boolean>("export_command_set_file", {
        suggestedName: `${activeConfig.name}.json`,
        content: JSON.stringify(activeConfig, null, 2),
      });
    } catch (error) {
      console.error("Export command set failed:", error);
      showToast("error", t("commandPanel.exportFailed"));
    }
  }, [runner.isRunning, activeConfig, showToast, t]);

  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <div className={styles.configActions}>
          <select
            className={`${styles.configSelect} liquid-glass-input liquid-glass-select`}
            value={activeConfigName}
            onChange={event => handleConfigChange(event.target.value)}
            title={t("commandPanel.switchConfig")}
            disabled={runner.isRunning}
          >
            {configs.map(config => (
              <option key={config.name} value={config.name}>{config.name}</option>
            ))}
          </select>
          <button className={`${styles.configBtn} liquid-glass-button`} onClick={handleAddConfig} title={t("sendBar.new")} disabled={runner.isRunning}>
            <Icon name="plus" size="sm" />
          </button>
          <button className={`${styles.configBtn} liquid-glass-button`} onClick={handleRenameStart} title={t("sendBar.rename")} disabled={runner.isRunning || !activeConfig}>
            <Icon name="edit" size="sm" />
          </button>
          <button
            className={`${styles.configBtn} liquid-glass-button ${styles.configBtnDanger}`}
            onClick={() => setConfigDeleteConfirm(true)}
            title={t("sendBar.delete")}
            disabled={runner.isRunning || !activeConfig}
          >
            <Icon name="trash" size="sm" />
          </button>
        </div>

        <div className={styles.toolbarActions}>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleLoadExamples(); }} title={t("sendBar.loadBuiltinExamples")} disabled={runner.isRunning}>
            {t("sendBar.loadBuiltinExamples")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleImport(); }} title={t("commandPanel.import")} disabled={runner.isRunning}>
            {t("commandPanel.import")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={() => { void handleExport(); }} title={t("commandPanel.export")} disabled={runner.isRunning || !activeConfig}>
            {t("commandPanel.export")}
          </button>
          <button className={`${styles.toolBtn} liquid-glass-button`} onClick={handleAdd} title={t("commandPanel.addCommand")} disabled={runner.isRunning || !activeConfig}>
            + {t("commandPanel.addCommand")}
          </button>
        </div>
      </div>

      <div ref={listRef} className={`${styles.commandList} ${isDragging ? styles.listDragging : ""}`}>
        {configs.length === 0 ? (
          <div className={styles.empty}>{t("commandPanel.noConfigs")}</div>
        ) : commands.length === 0 && (
          <div className={styles.empty}>{t("commandPanel.empty")}</div>
        )}
        {commands.map((command, index) => {
          const isSelected = selectedIds.has(command.id);
          const isCurrent = runner.isRunning && runner.currentIndex === index;
          return (
            <Fragment key={command.id}>
              {isDragging && dropIndex === index && <div className={styles.dropIndicator} />}
              <div className={`${styles.commandRow} ${isSelected ? styles.rowSelected : ""} ${isCurrent ? styles.rowRunning : ""}`}>
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
                    checked={isSelected}
                    onChange={() => toggleSelect(command.id)}
                    disabled={runner.isRunning}
                  />
                  <div className={styles.checkTrack} />
                </label>
                <code className={styles.commandText} title={command.command}>{command.command}</code>
                <span className={styles.commandNote}>{command.note}</span>
                <input
                  type="number"
                  className={`${styles.delayInput} liquid-glass-input`}
                  value={command.delay}
                  onChange={event => handleDelayChange(command.id, Number(event.target.value))}
                  min={0}
                  max={60000}
                  step={100}
                  title={t("commandPanel.delay")}
                  disabled={runner.isRunning}
                />
                <span className={styles.delayUnit}>ms</span>

                <button className={`${styles.editBtn} liquid-glass-button`} onClick={() => handleEdit(command)} title={t("sendBar.edit")} disabled={runner.isRunning}>
                  <Icon name="edit" size="sm" />
                </button>
                <button className={`${styles.deleteBtn} liquid-glass-button`} onClick={() => setDeleteConfirmId(command.id)} title={t("common.delete")} disabled={runner.isRunning}>
                  <Icon name="trash" size="sm" />
                </button>
              </div>
            </Fragment>
          );
        })}
        {isDragging && dropIndex === commands.length && commands.length > 0 && <div className={styles.dropIndicator} />}
      </div>

      <div className={styles.controlBar}>
        <span className={styles.status}>
          <Icon name={runner.isRunning ? "status-connected" : "status-idle"} size={10} />
          {runner.isRunning ? t("sendBar.running") : t("sendBar.idle")}
        </span>
        <div className={styles.controlSep} />

        <label className={styles.controlLabel}>
          <input
            type="checkbox"
            className={styles.checkInput}
            checked={selectedIds.size === commands.length && commands.length > 0}
            onChange={handleSelectAll}
            disabled={runner.isRunning}
          />
          <div className={styles.checkTrack} />
          <span>{t("commandPanel.selectAll")}</span>
        </label>
        <div className={styles.controlSep} />

        <label className={styles.controlLabel}>
          <Icon name="loop" size="xs" />
          <input
            type="number"
            className={`${styles.loopCountInput} liquid-glass-input`}
            value={loopCount}
            onChange={handleLoopCountChange}
            min={0}
            step={1}
            disabled={runner.isRunning}
            title={t("commandPanel.loopCount")}
          />
          <span>{loopCount === 0 ? t("commandPanel.infinite") : t("commandPanel.times")}</span>
        </label>
        <div className={styles.controlSep} />

        {runner.isRunning && runner.loopProgress && (
          <div className={styles.progressBar}>
            <div
              className={`${styles.progressFill} ${runner.loopProgress.total === -1 ? styles.progressInfinite : ""}`}
              style={runner.loopProgress.total !== -1
                ? { width: `${Math.min(100, (runner.loopProgress.current / runner.loopProgress.total) * 100)}%` }
                : undefined}
            />
          </div>
        )}

        <button
          className={`${styles.runBtn} ${runner.isRunning ? `${styles.stopBtn} ${styles.stopBtnWrap}` : "liquid-primary-button"}`}
          onClick={handleStart}
          disabled={!runner.isRunning && (!isConnected || selectedIds.size === 0)}
          title={runner.isRunning ? t("commandPanel.stopExecution") : t("commandPanel.start")}
        >
          {runner.isRunning
            ? <><Icon name="stop" size="xs" /> {t("commandPanel.stopExecution")}</>
            : <><Icon name="play" size="xs" /> {t("commandPanel.start")}</>}
        </button>
      </div>

      <CommandEditorModal
        isOpen={editorOpen && !runner.isRunning}
        editItem={editingItem}
        defaultDelay={defaultDelay}
        onSave={handleSaveCommand}
        onClose={() => setEditorOpen(false)}
      />

      {renameOpen && !runner.isRunning && createPortal(
        <div className={`${styles.modalOverlay} glass-overlay`} onClick={handleRenameCancel}>
          <div className={`${styles.renameModal} liquid-glass`} onClick={event => event.stopPropagation()}>
            <h3 className={styles.renameModalTitle}>{t("sendBar.renameTitle")}</h3>
            <input
              className={`${styles.renameModalInput} liquid-glass-input`}
              type="text"
              value={renameValue}
              onChange={event => setRenameValue(event.target.value)}
              onKeyDown={event => {
                if (event.key === "Enter") handleRenameConfirm();
                else if (event.key === "Escape") handleRenameCancel();
              }}
              placeholder={t("sendBar.renamePlaceholder")}
              autoFocus
            />
            <div className={styles.renameModalBtns}>
              <button className={`${styles.renameModalCancelBtn} liquid-glass-button`} onClick={handleRenameCancel}>
                {t("sendBar.cancel")}
              </button>
              <button className={`${styles.renameModalSaveBtn} liquid-primary-button`} onClick={handleRenameConfirm} disabled={!renameValue.trim()}>
                {t("sendBar.save")}
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )}

      <ConfirmDialog
        open={configDeleteConfirm}
        title={t("commandPanel.deleteConfigConfirm")}
        message={activeConfig?.name}
        intent="danger"
        size="compact"
        onConfirm={handleDeleteConfig}
        onCancel={() => setConfigDeleteConfirm(false)}
      />
      <ConfirmDialog
        open={deleteConfirmId !== null}
        title={t("commandPanel.confirmDelete")}
        message={deletingCommand?.command}
        intent="danger"
        size="compact"
        onConfirm={handleDeleteConfirmed}
        onCancel={() => setDeleteConfirmId(null)}
      />
    </div>
  );
}
