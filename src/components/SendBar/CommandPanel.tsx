import { useState, useCallback, useEffect, useMemo, useRef, Fragment } from "react";
import { useTranslation } from "react-i18next";
import { save, open } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { useSession } from "../../context/SessionContext";
import { useSendBar } from "./SendBarContext";
import Icon from "../common/Icon";
import CommandEditorModal from "./CommandEditorModal";
import useCommandRunner from "./useCommandRunner";
import defaultCommands from "./default-commands.json";
import type { CommandItem, CommandConfig } from "./types";
import styles from "./CommandPanel.module.css";

interface CommandPanelProps {
  sessionId: string;
  isActive: boolean;
  onRunningChange?: (running: boolean) => void;
}

const STORAGE_KEY_CONFIGS = "tauterm-command-configs";
const STORAGE_KEY_ACTIVE = "tauterm-active-command-config";

function loadConfigs(): CommandConfig[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_CONFIGS);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) return parsed as CommandConfig[];
    }
  } catch { /* ignore */ }
  const initial = [defaultCommands as CommandConfig];
  try {
    localStorage.setItem(STORAGE_KEY_CONFIGS, JSON.stringify(initial));
    localStorage.setItem(STORAGE_KEY_ACTIVE, initial[0].name);
  } catch { /* ignore */ }
  return initial;
}

function saveConfigs(configs: CommandConfig[]) {
  try {
    localStorage.setItem(STORAGE_KEY_CONFIGS, JSON.stringify(configs));
  } catch { /* ignore */ }
}

export default function CommandPanel({ sessionId, isActive, onRunningChange }: CommandPanelProps) {
  const { t } = useTranslation();
  const { sendData, state } = useSession();
  const activeTab = state.tabs.find(tab => tab.id === sessionId);
  const isConnected = activeTab?.state === "connected" || activeTab?.state === "transferring";

  const [configs, setConfigs] = useState<CommandConfig[]>(() => loadConfigs());
  const [activeConfigName, setActiveConfigName] = useState(() => {
    return localStorage.getItem(STORAGE_KEY_ACTIVE) || defaultCommands.name;
  });

  const activeConfig = useMemo(() => {
    return configs.find(c => c.name === activeConfigName) ?? configs[0];
  }, [configs, activeConfigName]);

  // `commands` 和 `defaultDelay` 保留为局部 state 而非常量 context：
  // commands 涉及拖拽排序和独立 localStorage 持久化，defaultDelay 与 commands 紧耦合；
  // 两者均不与 BasicSend 共享，迁入 context 会增加不必要的 dispatch 间接层。
  const [commands, setCommands] = useState<CommandItem[]>(activeConfig?.commands ?? []);
  const [defaultDelay, setDefaultDelay] = useState(activeConfig?.defaultDelay ?? 500);
  const { state: sendBarState, dispatch } = useSendBar();
  const { selectedIds, loopCount } = sendBarState.command;

  useEffect(() => {
    if (activeConfig) {
      setCommands(activeConfig.commands);
      setDefaultDelay(activeConfig.defaultDelay);
      dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    }
  }, [activeConfigName, activeConfig, dispatch]);

  const [editorOpen, setEditorOpen] = useState(false);
  const [editingItem, setEditingItem] = useState<CommandItem | null>(null);

  // 删除确认
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  // 命令集管理
  const [renameActive, setRenameActive] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [configDeleteConfirm, setConfigDeleteConfirm] = useState(false);

  // 拖拽排序（指针事件实现，兼容 Tauri WebView2）
  const dragIndexRef = useRef<number | null>(null);   // 当前拖拽项的索引（ref，避免闭包过期）
  const dropIndexRef = useRef<number | null>(null);   // 计算出的插入索引（ref）
  const [dropIndex, setDropIndex] = useState<number | null>(null); // UI 用：驱动放置指示线渲染
  const [isDragging, setIsDragging] = useState(false);            // UI 用：拖拽中状态
  const listRef = useRef<HTMLDivElement>(null);      // 命令列表容器 DOM ref

  const runner = useCommandRunner({
    onSend: useCallback(async (cmd: CommandItem) => {
      if (!isConnected) return;
      await sendData(sessionId, cmd.command + "\r\n");
    }, [sessionId, sendData, isConnected]),
  });

  // 通知父组件执行状态（用于锁定模式切换）
  const onRunningChangeRef = useRef(onRunningChange);
  onRunningChangeRef.current = onRunningChange;
  useEffect(() => {
    if (runner.isRunning && isActive) {
      onRunningChangeRef.current?.(true);
      return () => onRunningChangeRef.current?.(false);
    }
  }, [runner.isRunning, isActive]);

  const persistConfig = useCallback((cmds: CommandItem[], delay: number) => {
    setConfigs(prev => {
      const updated = prev.map(c =>
        c.name === activeConfigName
          ? { ...c, commands: cmds, defaultDelay: delay }
          : c
      );
      saveConfigs(updated);
      return updated;
    });
  }, [activeConfigName]);

  // ── 命令集管理 ──

  const handleConfigChange = useCallback((name: string) => {
    setActiveConfigName(name);
    localStorage.setItem(STORAGE_KEY_ACTIVE, name);
    setDeleteConfirmId(null);
    setConfigDeleteConfirm(false);
    if (runner.isRunning) runner.stop();
  }, [runner]);

  const handleRenameStart = useCallback(() => {
    setRenameValue(activeConfig?.name ?? "");
    setRenameActive(true);
  }, [activeConfig]);

  const handleRenameConfirm = useCallback(() => {
    const newName = renameValue.trim();
    if (!newName || newName === activeConfigName) {
      setRenameActive(false);
      return;
    }
    setConfigs(prev => {
      const updated = prev.map(c =>
        c.name === activeConfigName ? { ...c, name: newName } : c
      );
      saveConfigs(updated);
      return updated;
    });
    setActiveConfigName(newName);
    localStorage.setItem(STORAGE_KEY_ACTIVE, newName);
    setRenameActive(false);
  }, [renameValue, activeConfigName]);

  const handleRenameCancel = useCallback(() => {
    setRenameActive(false);
  }, []);

  const handleDeleteConfig = useCallback(() => {
    if (configs.length <= 1) return;
    if (!configDeleteConfirm) {
      setConfigDeleteConfirm(true);
      return;
    }
    // 确认删除
    setConfigs(prev => {
      const next = prev.filter(c => c.name !== activeConfigName);
      saveConfigs(next);
      return next;
    });
    // 切换到第一个剩余的命令集
    const remaining = configs.filter(c => c.name !== activeConfigName);
    if (remaining.length > 0) {
      setActiveConfigName(remaining[0].name);
      localStorage.setItem(STORAGE_KEY_ACTIVE, remaining[0].name);
    }
    setConfigDeleteConfirm(false);
    if (runner.isRunning) runner.stop();
  }, [configs, activeConfigName, configDeleteConfirm, runner]);

  const handleAddConfig = useCallback(() => {
    // 生成唯一名称
    let baseName = t("commandPanel.newConfigName") || "新命令集";
    let newName = baseName;
    let counter = 2;
    while (configs.some(c => c.name === newName)) {
      newName = `${baseName} (${counter})`;
      counter++;
    }
    const newConfig: CommandConfig = {
      version: 1,
      name: newName,
      defaultDelay: 500,
      commands: [],
    };
    setConfigs(prev => {
      const updated = [...prev, newConfig];
      saveConfigs(updated);
      return updated;
    });
    setActiveConfigName(newName);
    localStorage.setItem(STORAGE_KEY_ACTIVE, newName);
    setConfigDeleteConfirm(false);
    if (runner.isRunning) runner.stop();
  }, [configs, runner, t]);

  // 重置删除确认（切换焦点时）
  useEffect(() => {
    setConfigDeleteConfirm(false);
  }, [activeConfigName]);

  // ── 命令操作 ──

  const handleAdd = useCallback(() => {
    setEditingItem(null);
    setEditorOpen(true);
  }, []);

  const handleEdit = useCallback((item: CommandItem) => {
    setEditingItem(item);
    setEditorOpen(true);
  }, []);

  const handleDeleteConfirmed = useCallback((id: string) => {
    setCommands(prev => {
      const next = prev.filter(c => c.id !== id);
      persistConfig(next, defaultDelay);
      return next;
    });
    if (selectedIds.has(id)) {
      dispatch({ type: "TOGGLE_COMMAND_SELECT", id });
    }
    setDeleteConfirmId(null);
  }, [defaultDelay, persistConfig, selectedIds, dispatch]);

  const handleSaveCommand = useCallback((item: CommandItem) => {
    setCommands(prev => {
      const idx = prev.findIndex(c => c.id === item.id);
      let next: CommandItem[];
      if (idx >= 0) {
        next = [...prev];
        next[idx] = item;
      } else {
        next = [...prev, item];
      }
      persistConfig(next, defaultDelay);
      return next;
    });
  }, [defaultDelay, persistConfig]);

  const toggleSelect = useCallback((id: string) => {
    dispatch({ type: "TOGGLE_COMMAND_SELECT", id });
  }, [dispatch]);

  const handleLoopCountChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const val = Number(e.target.value);
    if (isNaN(val)) return;
    dispatch({ type: "SET_LOOP_COUNT", count: Math.max(0, val) });
  }, [dispatch]);

  // ── 拖拽排序（指针事件实现，兼容 Tauri WebView2）──
  // WebView2 默认 dragDropEnabled: true 会在 OS 层拦截 HTML5 DnD 事件，
  // 因此使用 Pointer Events 完全替代 dragstart/dragover/drop。

  // 根据 pointer 的 clientY 计算插入索引（半区判断）
  const calcDropIndex = useCallback((clientY: number): number => {
    const rows = listRef.current?.querySelectorAll<HTMLElement>(`.${styles.commandRow}`);
    if (!rows || rows.length === 0) return 0;
    for (let i = 0; i < rows.length; i++) {
      const rect = rows[i].getBoundingClientRect();
      const midY = rect.top + rect.height / 2;
      if (clientY < midY) return i;
    }
    return rows.length;
  }, [styles.commandRow]);

  const handlePointerDown = useCallback((e: React.PointerEvent, index: number) => {
    if (runner.isRunning || !e.isPrimary) return;
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    handle.setPointerCapture(e.pointerId);
    dragIndexRef.current = index;
    setIsDragging(true);
    // 给拖拽行添加半透明效果
    const row = handle.closest(`.${styles.commandRow}`);
    if (row) {
      row.classList.add(styles.rowDragging);
    }
    // 初始指示线位置
    dropIndexRef.current = index;
    setDropIndex(index);
  }, [runner.isRunning, styles.commandRow, styles.rowDragging]);

  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (dragIndexRef.current === null) return;
    e.preventDefault();
    const newIndex = calcDropIndex(e.clientY);
    if (newIndex !== dropIndexRef.current) {
      dropIndexRef.current = newIndex;
      setDropIndex(newIndex);
    }
  }, [calcDropIndex]);

  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    const fromIndex = dragIndexRef.current;
    if (fromIndex === null) return;

    const handle = e.currentTarget as HTMLElement;
    handle.releasePointerCapture(e.pointerId);

    // 移除拖拽行视觉效果
    const row = handle.closest(`.${styles.commandRow}`);
    if (row) {
      row.classList.remove(styles.rowDragging);
    }

    // 执行排序
    const targetIndex = dropIndexRef.current;
    if (targetIndex !== null && targetIndex !== fromIndex) {
      const adjustedIndex = fromIndex < targetIndex ? targetIndex - 1 : targetIndex;
      if (fromIndex !== adjustedIndex) {
        setCommands(prev => {
          const next = [...prev];
          const [removed] = next.splice(fromIndex, 1);
          next.splice(adjustedIndex, 0, removed);
          persistConfig(next, defaultDelay);
          return next;
        });
      }
    }

    // 清理
    dragIndexRef.current = null;
    dropIndexRef.current = null;
    setDropIndex(null);
    setIsDragging(false);
  }, [styles.commandRow, styles.rowDragging, defaultDelay, persistConfig]);

  const handlePointerCancel = useCallback((e: React.PointerEvent) => {
    // 浏览器取消了 pointer（如手势冲突），清理状态
    if (dragIndexRef.current === null) return;
    const handle = e.currentTarget as HTMLElement;
    try { handle.releasePointerCapture(e.pointerId); } catch { /* already released */ }
    const row = handle.closest(`.${styles.commandRow}`);
    if (row) row.classList.remove(styles.rowDragging);
    dragIndexRef.current = null;
    dropIndexRef.current = null;
    setDropIndex(null);
    setIsDragging(false);
  }, [styles.commandRow, styles.rowDragging]);

  // ── 执行 ──

  const handleStart = useCallback(() => {
    const selected = commands.filter(c => selectedIds.has(c.id));
    if (selected.length === 0) return;
    if (runner.isRunning) {
      runner.stop();
    } else {
      runner.start(selected, loopCount);
    }
  }, [commands, selectedIds, loopCount, runner]);

  useEffect(() => {
    if ((!isConnected || !isActive) && runner.isRunning) {
      runner.stop();
    }
  }, [isConnected, isActive, runner]);

  const handleSelectAll = useCallback(() => {
    if (selectedIds.size === commands.length) {
      dispatch({ type: "CLEAR_COMMAND_SELECTION" });
    } else {
      dispatch({ type: "SELECT_ALL_COMMANDS", ids: commands.map(c => c.id) });
    }
  }, [commands, selectedIds, dispatch]);

  // ── 导入导出 ──

  const handleImport = useCallback(async () => {
    try {
      const selected = await open({
        filters: [{ name: "JSON", extensions: ["json"] }],
        multiple: false,
      });
      if (!selected) return;
      const content = await readTextFile(selected as string);
      const imported = JSON.parse(content) as CommandConfig;
      if (!imported.version || !Array.isArray(imported.commands)) {
        throw new Error("无效的配置文件格式");
      }
      // 重名则追加后缀，直接在闭包中用 configs 计算
      let importName = imported.name;
      if (configs.some(c => c.name === importName)) {
        importName = importName + " (导入)";
      }
      let counter = 2;
      while (configs.some(c => c.name === importName)) {
        importName = `${imported.name} (导入 ${counter})`;
        counter++;
      }
      const newConfig = { ...imported, name: importName };
      const updated = [...configs, newConfig];
      setConfigs(updated);
      saveConfigs(updated);
      setActiveConfigName(importName);
      localStorage.setItem(STORAGE_KEY_ACTIVE, importName);
    } catch (e) {
      console.error("导入失败:", e);
    }
  }, [configs]);

  const handleExport = useCallback(async () => {
    try {
      const selected = await save({
        filters: [{ name: "JSON", extensions: ["json"] }],
        defaultPath: `${activeConfig?.name ?? "commands"}.json`,
      });
      if (!selected) return;
      const config: CommandConfig = {
        version: 1,
        name: activeConfig?.name ?? "commands",
        defaultDelay,
        commands,
      };
      await writeTextFile(selected, JSON.stringify(config, null, 2));
    } catch (e) {
      console.error("导出失败:", e);
    }
  }, [activeConfig, defaultDelay, commands]);

  const handleDelayChange = useCallback((id: string, newDelay: number) => {
    setCommands(prev => {
      const next = prev.map(c => c.id === id ? { ...c, delay: Math.max(0, newDelay) } : c);
      persistConfig(next, defaultDelay);
      return next;
    });
  }, [defaultDelay, persistConfig]);

  useEffect(() => {
    persistConfig(commands, defaultDelay);
  }, [defaultDelay]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className={styles.panel}>
      {/* 工具栏 */}
      <div className={styles.toolbar}>
        {/* 左侧：命令集管理 */}
        <div className={styles.configActions}>
          {renameActive ? (
            <div className={styles.renameArea}>
              <input
                className={styles.renameInput}
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleRenameConfirm();
                  if (e.key === "Escape") handleRenameCancel();
                }}
                placeholder={t("commandPanel.renamePlaceholder") || "输入新名称"}
                autoFocus
                disabled={runner.isRunning}
              />
              <button
                className={styles.renameConfirmBtn}
                onClick={handleRenameConfirm}
                disabled={runner.isRunning}
              >
                <Icon name="check-plain" size="xs" />
              </button>
              <button
                className={styles.renameCancelBtn}
                onClick={handleRenameCancel}
                disabled={runner.isRunning}
              >
                <Icon name="close" size="xs" />
              </button>
            </div>
          ) : (
            <>
              <select
                className={styles.configSelect}
                value={activeConfigName}
                onChange={(e) => handleConfigChange(e.target.value)}
                title={t("commandPanel.switchConfig") || "切换命令集"}
                disabled={runner.isRunning}
              >
                {configs.map(c => (
                  <option key={c.name} value={c.name}>{c.name}</option>
                ))}
              </select>
              <button
                className={styles.configBtn}
                onClick={handleRenameStart}
                title={t("commandPanel.renameConfig") || "重命名"}
                disabled={runner.isRunning}
              >
                <Icon name="edit" size="xs" />
              </button>
              <button
                className={`${styles.configBtn} ${configs.length > 1 ? styles.configBtnDanger : ""}`}
                onClick={handleDeleteConfig}
                title={configDeleteConfirm
                  ? (t("commandPanel.deleteConfigConfirm") || "确认删除此命令集？")
                  : (t("commandPanel.deleteConfig") || "删除命令集")}
                disabled={runner.isRunning || configs.length <= 1}
              >
                {configDeleteConfirm ? <Icon name="warning" size="xs" /> : <Icon name="trash" size="xs" />}
              </button>
              <button
                className={styles.configBtn}
                onClick={handleAddConfig}
                title={t("commandPanel.newConfig") || "新建命令集"}
                disabled={runner.isRunning}
              >
                <Icon name="plus" size="xs" />
              </button>
            </>
          )}
        </div>

        {/* 右侧：导入/导出/新增命令 */}
        <div className={styles.toolbarActions}>
          <button
            className={`${styles.toolBtn} liquid-glass-button`}
            onClick={handleImport}
            title={t("commandPanel.import") || "导入"}
            disabled={runner.isRunning}
          >
            {t("commandPanel.import") || "导入"}
          </button>
          <button
            className={`${styles.toolBtn} liquid-glass-button`}
            onClick={handleExport}
            title={t("commandPanel.export") || "导出"}
            disabled={runner.isRunning}
          >
            {t("commandPanel.export") || "导出"}
          </button>
          <button
            className={`${styles.toolBtn} liquid-glass-button`}
            onClick={handleAdd}
            title={t("commandPanel.addCommand") || "新增命令"}
            disabled={runner.isRunning}
          >
            + {t("commandPanel.addCommand") || "新增命令"}
          </button>
        </div>
      </div>

      {/* 命令列表 */}
      <div
        ref={listRef}
        className={`${styles.commandList} ${isDragging ? styles.listDragging : ""}`}
      >
        {commands.length === 0 && (
          <div className={styles.empty}>
            {t("commandPanel.empty") || "暂无命令，点击「+ 新增命令」添加"}
          </div>
        )}
        {commands.map((cmd, i) => {
          const isSelected = selectedIds.has(cmd.id);
          const isCurrent = runner.isRunning && runner.currentIndex === i;
          const confirming = deleteConfirmId === cmd.id;
          return (
            <Fragment key={cmd.id}>
              {/* 放置位置指示线 — 拖拽时计算插入位置 */}
              {isDragging && dropIndex === i && (
                <div className={styles.dropIndicator} />
              )}
              <div
                className={`${styles.commandRow} ${isSelected ? styles.rowSelected : ""} ${isCurrent ? styles.rowRunning : ""} ${confirming ? styles.rowConfirming : ""}`}
              >
                {/* 拖拽把手 — 指针事件驱动拖拽 */}
                <span
                  className={styles.dragHandle}
                  title={t("commandPanel.dragToReorder") || "拖拽排序"}
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
                    checked={isSelected}
                    onChange={() => toggleSelect(cmd.id)}
                    disabled={runner.isRunning}
                  />
                  <div className={styles.checkTrack} />
                </label>
                <code
                  className={styles.commandText}
                  title={t("commandPanel.doubleClickToEdit") || "双击编辑"}
                  onDoubleClick={() => !runner.isRunning && handleEdit(cmd)}
                >
                  {cmd.command}
                </code>
                <span className={styles.commandNote}>{cmd.note}</span>
                <input
                  type="number"
                  className={`${styles.delayInput} liquid-glass-input`}
                  value={cmd.delay}
                  onChange={(e) => handleDelayChange(cmd.id, Number(e.target.value))}
                  min={0}
                  max={60000}
                  step={100}
                  title={t("commandPanel.delay") || "延时 (ms)"}
                  disabled={runner.isRunning}
                />
                <span className={styles.delayUnit}>ms</span>

                {confirming ? (
                  <div className={styles.confirmBox}>
                    <span className={styles.confirmText}>{t("commandPanel.confirmDelete") || "确认删除?"}</span>
                    <button
                      className={`${styles.confirmBtn} liquid-glass-button`}
                      onClick={() => handleDeleteConfirmed(cmd.id)}
                    >
                      {t("common.confirm")}
                    </button>
                    <button
                      className={`${styles.confirmBtn} liquid-glass-button`}
                      onClick={() => setDeleteConfirmId(null)}
                    >
                      {t("common.cancel")}
                    </button>
                  </div>
                ) : (
                  <button
                    className={`${styles.deleteBtn} liquid-glass-button`}
                    onClick={() => setDeleteConfirmId(cmd.id)}
                    title={t("common.delete") || "删除"}
                    disabled={runner.isRunning}
                  >
                    <Icon name="trash" size="xs" />
                  </button>
                )}
              </div>
            </Fragment>
          );
        })}
        {/* 拖到列表末尾时的指示线 */}
        {isDragging && dropIndex === commands.length && commands.length > 0 && (
          <div className={styles.dropIndicator} />
        )}
      </div>

      {/* 控制栏 */}
      <div className={styles.controlBar}>
        <label className={styles.controlLabel}>
          <input
            type="checkbox"
            className={styles.checkInput}
            checked={selectedIds.size === commands.length && commands.length > 0}
            onChange={handleSelectAll}
            disabled={runner.isRunning}
          />
          <div className={styles.checkTrack} />
          <span>{t("commandPanel.selectAll") || "全选"}</span>
        </label>

        <div className={styles.controlSep} />

        {/* 循环次数输入 */}
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

        {/* 进度条 */}
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
          disabled={!isConnected || selectedIds.size === 0}
          title={
            runner.isRunning
              ? (t("commandPanel.stopExecution") || "停止执行")
              : (t("commandPanel.start") || "开始执行")
          }
        >
          {runner.isRunning
            ? <><Icon name="stop" size="xs" /> {t("commandPanel.stopExecution") || "停止执行"}</>
            : <><Icon name="play" size="xs" /> {t("commandPanel.start") || "开始执行"}</>
          }
        </button>
      </div>

      <CommandEditorModal
        isOpen={editorOpen}
        editItem={editingItem}
        defaultDelay={defaultDelay}
        onSave={handleSaveCommand}
        onClose={() => setEditorOpen(false)}
      />
    </div>
  );
}
