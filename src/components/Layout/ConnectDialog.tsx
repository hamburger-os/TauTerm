import { useState, useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { useSession } from "../../context/SessionContext";
import {
  pluginRegistry,
  type PluginRegistration,
  type SessionConnectOptions,
} from "../../core/plugin-registry";
import Icon from "../common/Icon";
import styles from "./ConnectDialog.module.css";

interface ConnectDialogProps {
  isOpen: boolean;
  onClose: () => void;
  editSessionId?: string | null;
}

const EMPTY_SESSION_OPTIONS: SessionConnectOptions = {
  transferEnabled: false,
  sendBarEnabled: false,
};

function defaultSessionOptions(plugin: PluginRegistration | undefined): SessionConnectOptions {
  if (!plugin) return { ...EMPTY_SESSION_OPTIONS };
  return plugin.defaultSessionOptions?.() ?? {
    transferEnabled: false,
    transferProtocol: plugin.manifest.transfer_protocols[0],
    sendBarEnabled: plugin.manifest.send_bar,
  };
}

/**
 * 通用 Session 配置宿主。
 *
 * ConnectDialog 只负责选择插件、承载插件表单和提交通用 Session 草稿；协议字段、默认值、
 * 校验、endpoint 解析和提交前准备全部属于 PluginRegistration contribution。
 */
export default function ConnectDialog({ isOpen, onClose, editSessionId }: ConnectDialogProps) {
  const { t } = useTranslation();
  const { state, refreshEndpoints, switchTab, createOfflineSession, reconfigureSession } = useSession();
  const [step, setStep] = useState<"mode" | "config">("mode");
  const [selectedMode, setSelectedMode] = useState("");
  const [pluginParams, setPluginParams] = useState<Record<string, unknown>>({});
  const [endpoint, setEndpoint] = useState("");
  const [sessionOptions, setSessionOptions] = useState<SessionConnectOptions>(EMPTY_SESSION_OPTIONS);
  const [sessionName, setSessionName] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [refreshingEndpoints, setRefreshingEndpoints] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedPlugin = pluginRegistry.get(selectedMode);
  const PluginConnectForm = selectedPlugin?.connectForm;
  const modeEndpoints = state.endpoints.filter(item => item.connection_type === selectedMode);
  const pluginConnectionConfigValid = selectedPlugin?.isConnectionConfigValid?.(pluginParams, endpoint) !== false;
  const canResolveEndpoint = Boolean(selectedPlugin?.resolveEndpoint || endpoint.trim());
  const canSubmit = Boolean(PluginConnectForm && pluginConnectionConfigValid && canResolveEndpoint && !connecting);

  const tabsRef = useRef(state.tabs);
  tabsRef.current = state.tabs;
  const endpointRefreshRequestRef = useRef(0);

  const availableModes = pluginRegistry.getByCapability("connection").map(plugin => ({
    id: plugin.manifest.id,
    icon: plugin.manifest.icon,
    description: plugin.manifest.description || plugin.manifest.name,
  }));

  const refreshModeEndpoints = useCallback(async (modeId: string, force = false) => {
    const requestId = ++endpointRefreshRequestRef.current;
    setRefreshingEndpoints(true);
    try {
      await refreshEndpoints(modeId, force);
    } finally {
      if (endpointRefreshRequestRef.current === requestId) setRefreshingEndpoints(false);
    }
  }, [refreshEndpoints]);

  useEffect(() => {
    if (!isOpen) {
      ++endpointRefreshRequestRef.current;
      setRefreshingEndpoints(false);
      setPluginParams({});
      setEndpoint("");
      setSessionName("");
      setSessionOptions({ ...EMPTY_SESSION_OPTIONS });
      setStep("mode");
      return;
    }

    setError(null);
    setConnecting(false);
    if (editSessionId) {
      const tab = tabsRef.current.find(item => item.id === editSessionId);
      if (tab) {
        const plugin = pluginRegistry.get(tab.pluginId);
        const params = tab.params ?? {};
        const defaults = defaultSessionOptions(plugin);
        setSelectedMode(tab.pluginId);
        setPluginParams(plugin?.normalizeConnectionParams?.(params) ?? params);
        setEndpoint(tab.endpoint);
        setSessionOptions({
          transferEnabled: tab.transferEnabled ?? defaults.transferEnabled,
          transferProtocol: tab.transferProtocol ?? defaults.transferProtocol,
          sendBarEnabled: tab.sendBarEnabled ?? defaults.sendBarEnabled,
        });
        setSessionName(tab.name);
        setStep("config");
        return;
      }
    }

    setSelectedMode("");
    setPluginParams({});
    setEndpoint("");
    setSessionOptions({ ...EMPTY_SESSION_OPTIONS });
    setSessionName("");
    setStep("mode");
  }, [isOpen, editSessionId]);

  useEffect(() => {
    if (!isOpen || step !== "config") return;
    if (editSessionId) {
      const tab = tabsRef.current.find(item => item.id === editSessionId);
      if (!tab || tab.pluginId !== selectedMode) return;
    }
    if (!selectedPlugin?.manifest.capabilities.includes("endpoint_discovery")) return;
    void refreshModeEndpoints(selectedMode);
  }, [isOpen, step, selectedMode, editSessionId, selectedPlugin, refreshModeEndpoints]);

  useEffect(() => {
    if (!isOpen || step !== "config" || editSessionId) return;
    if (selectedPlugin?.resolveEndpoint || endpoint || modeEndpoints.length === 0) return;
    setEndpoint(modeEndpoints[0].name);
  }, [isOpen, step, editSessionId, selectedPlugin, endpoint, modeEndpoints]);

  const handleModeSelect = useCallback((modeId: string) => {
    const plugin = pluginRegistry.get(modeId);
    setSelectedMode(modeId);
    setPluginParams(plugin?.defaultConnectionParams?.() ?? {});
    setEndpoint("");
    setSessionOptions(defaultSessionOptions(plugin));
    setSessionName("");
    setStep("config");
    setError(null);
  }, []);

  const handleBack = useCallback(() => {
    setStep("mode");
    setPluginParams({});
    setEndpoint("");
    setSessionOptions({ ...EMPTY_SESSION_OPTIONS });
    setSessionName("");
    setError(null);
  }, []);

  const handleCreate = useCallback(async () => {
    const plugin = pluginRegistry.get(selectedMode);
    if (!plugin?.connectForm || plugin.isConnectionConfigValid?.(pluginParams, endpoint) === false) return;

    setError(null);
    setConnecting(true);
    try {
      const prepared = await plugin.prepareConnectionParams?.(pluginParams) ?? pluginParams;
      const params = plugin.normalizeConnectionParams?.(prepared) ?? prepared;
      if (plugin.isConnectionConfigValid?.(params, endpoint) === false) {
        throw new Error(t("session.invalidConfiguration", { defaultValue: "Invalid session configuration" }));
      }

      const resolvedEndpoint = String(
        await plugin.resolveEndpoint?.(params, endpoint) ?? endpoint,
      ).trim();
      if (!resolvedEndpoint) {
        throw new Error(t("session.endpointRequired", { defaultValue: "A connection endpoint is required" }));
      }

      const transferEnabled = sessionOptions.transferEnabled;
      const transferProtocol = transferEnabled ? sessionOptions.transferProtocol : undefined;
      const sendBarEnabled = pluginRegistry.resolveSendBarEnabled(
        selectedMode,
        sessionOptions.sendBarEnabled,
      );
      const requestedName = sessionName.trim() || undefined;

      if (editSessionId) {
        await reconfigureSession(
          editSessionId,
          resolvedEndpoint,
          params,
          requestedName,
          transferEnabled,
          transferProtocol,
          sendBarEnabled,
          selectedMode,
        );
        onClose();
        return;
      }

      const sessionId = await createOfflineSession(
        resolvedEndpoint,
        params,
        requestedName,
        selectedMode,
        transferEnabled,
        transferProtocol,
        sendBarEnabled,
      );
      if (sessionId) {
        await switchTab(sessionId);
        onClose();
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setConnecting(false);
    }
  }, [
    selectedMode,
    pluginParams,
    endpoint,
    sessionOptions,
    sessionName,
    editSessionId,
    reconfigureSession,
    createOfflineSession,
    switchTab,
    onClose,
    t,
  ]);

  const handleOverlayClick = useCallback((event: React.MouseEvent) => {
    if (event.target === event.currentTarget) onClose();
  }, [onClose]);

  useEffect(() => {
    if (!isOpen) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          data-testid="connect-dialog-overlay"
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.18, ease: [0.4, 0, 0.2, 1] }}
          onClick={handleOverlayClick}
        >
          <motion.div
            initial={{ y: 20, scale: 0.95, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: 20, scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.15, delay: 0.05, ease: [0.4, 0, 0.2, 1] }}
          >
            <div className={`${styles.dialog} liquid-glass`}>
              {step === "mode" ? (
                <>
                  <h2 className={styles.title}>{t("session.newSession")}</h2>
                  <p className={styles.subtitle}>{t("connectionType.label")}</p>
                  <div className={styles.modeGrid}>
                    {availableModes.map(mode => (
                      <motion.button
                        key={mode.id}
                        className={`${styles.modeCard} liquid-glass-card`}
                        whileHover={{ scale: 1.03, borderColor: "var(--accent-primary)" }}
                        whileTap={{ scale: 0.97 }}
                        onClick={() => handleModeSelect(mode.id)}
                      >
                        <Icon name={mode.icon} size="lg" className={styles.modeIcon} />
                        <span className={styles.modeLabel}>{mode.description}</span>
                      </motion.button>
                    ))}
                  </div>
                  <div className={styles.actions}>
                    <button className={`${styles.cancelBtn} liquid-glass-button`} onClick={onClose}>
                      {t("common.cancel")}
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div className={styles.configHeader}>
                    {!editSessionId && (
                      <button className={`${styles.backBtn} liquid-glass-button`} onClick={handleBack} disabled={connecting}>
                        <Icon name="arrow-left" size="sm" /> {t("common.back")}
                      </button>
                    )}
                    <h2 className={styles.title}>
                      {selectedPlugin && <><Icon name={selectedPlugin.manifest.icon} size="md" />{" "}{selectedPlugin.manifest.name}</>}
                    </h2>
                  </div>

                  <div className={styles.field}>
                    <label className={styles.label}>{t("session.renameSession")} ({t("session.newSession")})</label>
                    <input
                      className={`${styles.input} liquid-glass-input`}
                      type="text"
                      placeholder={selectedPlugin?.manifest.name ?? "Session"}
                      value={sessionName}
                      onChange={event => setSessionName(event.target.value)}
                      disabled={connecting}
                    />
                  </div>

                  {PluginConnectForm ? (
                    <PluginConnectForm
                      params={pluginParams}
                      onChange={setPluginParams}
                      endpoints={modeEndpoints}
                      endpoint={endpoint}
                      onEndpointChange={setEndpoint}
                      onRefreshEndpoints={selectedPlugin?.manifest.capabilities.includes("endpoint_discovery")
                        ? () => void refreshModeEndpoints(selectedMode, true)
                        : undefined}
                      refreshingEndpoints={refreshingEndpoints}
                      disabled={connecting}
                      sessionOptions={sessionOptions}
                      onSessionOptionsChange={setSessionOptions}
                    />
                  ) : (
                    <div className={styles.comingSoonBanner} style={{ marginTop: 16 }}>
                      <Icon name="construction" size="lg" />{" "}
                      {t("connectionType.formNotImplemented", { pluginName: selectedPlugin?.manifest.name ?? selectedMode })}
                    </div>
                  )}

                  {error && <div className={styles.error}>{error}</div>}

                  <div className={styles.actions}>
                    <button className={`${styles.cancelBtn} liquid-glass-button`} onClick={onClose} disabled={connecting}>
                      {t("common.cancel")}
                    </button>
                    <button
                      className={`${styles.connectBtn} liquid-primary-button`}
                      onClick={() => void handleCreate()}
                      disabled={!canSubmit}
                    >
                      {connecting
                        ? t("common.confirming", { defaultValue: "Saving..." })
                        : t("common.confirm")}
                    </button>
                  </div>
                </>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
