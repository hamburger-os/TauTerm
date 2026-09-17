import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import Icon from "../../components/common/Icon";
import styles from "./TrdpConnectForm.module.css";
import {
  STANDARD_CAPTURE_FILTER,
  captureFilterForPorts,
  captureInterfaceRef,
  monitorCaptureInterfaces,
  type CaptureInterface,
} from "./model";

function str(params: Record<string, unknown>, key: string, fallback = "") {
  const value = params[key];
  return typeof value === "string" ? value : fallback;
}

function bool(params: Record<string, unknown>, key: string, fallback = false) {
  const value = params[key];
  return typeof value === "boolean" ? value : fallback;
}

function num(params: Record<string, unknown>, key: string, fallback: number) {
  const value = params[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export default function TrdpConnectForm({ params, onChange }: ConnectFormProps) {
  const { t } = useTranslation();
  const mode = str(params, "mode", "node") as "node" | "monitor";
  const [captureInterfaces, setCaptureInterfaces] = useState<CaptureInterface[]>([]);
  const [captureInterfacesLoading, setCaptureInterfacesLoading] = useState(false);
  const [captureInterfacesError, setCaptureInterfacesError] = useState<string | null>(null);
  const captureConfig = monitorCaptureInterfaces(params);
  const captureInterfaceA = captureConfig.a;
  const captureInterfaceB = captureConfig.b;
  const [captureInterfaceBEditorEnabled, setCaptureInterfaceBEditorEnabled] = useState(
    captureInterfaceB !== null,
  );

  useEffect(() => {
    if (captureInterfaceB !== null) {
      setCaptureInterfaceBEditorEnabled(true);
    }
  }, [captureInterfaceB?.deviceName]);

  const patch = (next: Record<string, unknown>) => {
    const {
      capture_interface: _obsoleteCaptureInterface,
      capture_interface_b_enabled: _obsoleteCaptureInterfaceBEnabled,
      capture_interface_b: _obsoleteCaptureInterfaceB,
      ...currentParams
    } = params;
    void _obsoleteCaptureInterface;
    void _obsoleteCaptureInterfaceBEnabled;
    void _obsoleteCaptureInterfaceB;
    onChange({
      mode: "node",
      link_a_ip: "0.0.0.0",
      link_b_enabled: false,
      link_b_ip: "0.0.0.0",
      pd_port: 17224,
      md_udp_port: 17225,
      md_tcp_port: 17225,
      capture_interfaces: { a: null, b: null },
      capture_filter_auto: bool(
        params,
        "capture_filter_auto",
        str(params, "capture_filter", STANDARD_CAPTURE_FILTER) === STANDARD_CAPTURE_FILTER,
      ),
      capture_filter: STANDARD_CAPTURE_FILTER,
      ...currentParams,
      ...next,
    });
  };

  async function refreshCaptureInterfaces() {
    setCaptureInterfacesLoading(true);
    setCaptureInterfacesError(null);
    try {
      const items = await invoke<CaptureInterface[]>("trdp_capture_interfaces");
      setCaptureInterfaces(items);
      if (items.length === 0) {
        setCaptureInterfacesError(t("trdp.captureInterfaces.empty"));
      }
    } catch (cause) {
      setCaptureInterfaces([]);
      setCaptureInterfacesError(`${t("trdp.captureInterfaces.error")}: ${String(cause)}`);
    } finally {
      setCaptureInterfacesLoading(false);
    }
  }

  useEffect(() => {
    if (mode !== "monitor") return;
    void refreshCaptureInterfaces();
  }, [mode]);

  async function chooseXml() {
    const path = await open({
      multiple: false,
      filters: [{ name: "TRDP XML", extensions: ["xml"] }],
    });
    if (typeof path === "string") patch({ xml_path: path });
  }

  const pdPort = num(params, "pd_port", 17224);
  const mdUdpPort = num(params, "md_udp_port", 17225);
  const mdTcpPort = num(params, "md_tcp_port", 17225);
  const captureFilterAuto = bool(
    params,
    "capture_filter_auto",
    str(params, "capture_filter", STANDARD_CAPTURE_FILTER) === STANDARD_CAPTURE_FILTER,
  );
  const captureFilter = str(params, "capture_filter", STANDARD_CAPTURE_FILTER);
  const captureInterfaceAKnown = captureInterfaces.some(
    item => item.name === captureInterfaceA?.deviceName,
  );
  const captureInterfaceBKnown = captureInterfaces.some(
    item => item.name === captureInterfaceB?.deviceName,
  );

  const portFields = (
    <div className={styles.ports}>
      <div className={styles.field}>
        <label className={styles.label}>{t("trdp.form.pdUdpPort")}</label>
        <input
          className={`${styles.numberInput} liquid-glass-input`}
          type="number"
          min={1}
          max={65535}
          value={pdPort}
          onChange={e => patch({ pd_port: Number(e.target.value) })}
        />
      </div>
      <div className={styles.field}>
        <label className={styles.label}>{t("trdp.form.mdUdpPort")}</label>
        <input
          className={`${styles.numberInput} liquid-glass-input`}
          type="number"
          min={1}
          max={65535}
          value={mdUdpPort}
          onChange={e => patch({ md_udp_port: Number(e.target.value) })}
        />
      </div>
      <div className={styles.field}>
        <label className={styles.label}>{t("trdp.form.mdTcpPort")}</label>
        <input
          className={`${styles.numberInput} liquid-glass-input`}
          type="number"
          min={1}
          max={65535}
          value={mdTcpPort}
          onChange={e => patch({ md_tcp_port: Number(e.target.value) })}
        />
      </div>
    </div>
  );

  return (
    <div className={styles.root}>
      <div className={styles.field}>
        <label className={styles.label}>{t("trdp.form.sessionMode")}</label>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={mode}
          onChange={e => patch({ mode: e.target.value })}
        >
          <option value="node">{t("trdp.form.nodeOption")}</option>
          <option value="monitor">{t("trdp.form.monitorOption")}</option>
        </select>
      </div>

      {mode === "node" ? (
        <>
          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.linkALocalIp")}</label>
            <input
              className={`${styles.input} liquid-glass-input`}
              value={str(params, "link_a_ip", "0.0.0.0")}
              onChange={e => patch({ link_a_ip: e.target.value })}
              placeholder="10.0.0.10"
            />
            <small className={styles.hint}>{t("trdp.form.linkAHint")}</small>
          </div>

          <label className={`liquid-glass-toggle ${styles.toggle}`}>
            <input
              type="checkbox"
              checked={bool(params, "link_b_enabled")}
              onChange={e => patch({ link_b_enabled: e.target.checked })}
            />
            <div />
            <span>{t("trdp.form.enableLinkB")}</span>
          </label>

          {bool(params, "link_b_enabled") && (
            <div className={styles.field}>
              <label className={styles.label}>{t("trdp.form.linkBLocalIp")}</label>
              <input
                className={`${styles.input} liquid-glass-input`}
                value={str(params, "link_b_ip", "0.0.0.0")}
                onChange={e => patch({ link_b_ip: e.target.value })}
                placeholder="10.0.1.10"
              />
            </div>
          )}

          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.xmlOptional")}</label>
            <div className={styles.pathRow}>
              <input
                className={`${styles.input} ${styles.pathInput} liquid-glass-input`}
                value={str(params, "xml_path")}
                onChange={e => patch({ xml_path: e.target.value })}
                placeholder="C:\\project\\trdp_config.xml"
              />
              <button
                type="button"
                className={`${styles.iconButton} liquid-glass-button`}
                onClick={() => void chooseXml()}
                title={t("trdp.form.chooseXml")}
                aria-label={t("trdp.form.chooseXml")}
              >
                <Icon name="folder" size="md" />
              </button>
            </div>
            <small className={styles.hint}>{t("trdp.form.xmlHint")}</small>
          </div>

          <details className={`${styles.details} liquid-glass-card`}>
            <summary className={styles.detailsSummary}><Icon name="chevron-right" size="xs" className={styles.detailsChevron} />{t("trdp.actions.advanced")}</summary>
            <div className={styles.detailsBody}>{portFields}</div>
          </details>
        </>
      ) : (
        <>
          <div className={`${styles.monitorIntro} liquid-glass-card`}>
            <strong>{t("trdp.form.monitorWorkspace")}</strong>
            <p>{t("trdp.form.monitorNote")}</p>
          </div>

          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.captureInterfaceA")}</label>
            <div className={styles.pathRow}>
              <select
                className={`${styles.select} ${styles.pathInput} liquid-glass-input liquid-glass-select`}
                value={captureInterfaceA?.deviceName ?? ""}
                onChange={event => {
                  const selected = captureInterfaces.find(item => item.name === event.target.value);
                  const nextA = selected ? captureInterfaceRef(selected) : null;
                  const nextB = nextA?.deviceName === captureInterfaceB?.deviceName
                    ? null
                    : captureInterfaceB;
                  if (nextB === null && captureInterfaceB !== null) {
                    setCaptureInterfaceBEditorEnabled(false);
                  }
                  patch({ capture_interfaces: { a: nextA, b: nextB } });
                }}
                disabled={captureInterfacesLoading}
              >
                <option value="">{captureInterfacesLoading ? t("trdp.captureInterfaces.loading") : t("trdp.captureInterfaces.choose")}</option>
                {captureInterfaceA && !captureInterfaceAKnown && (
                  <option value={captureInterfaceA.deviceName}>{captureInterfaceA.displayName}</option>
                )}
                {captureInterfaces.map(item => (
                  <option key={item.name} value={item.name}>
                    {item.description ? `${item.description} — ${item.name}` : item.name}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className={`${styles.iconButton} liquid-glass-button`}
                onClick={() => void refreshCaptureInterfaces()}
                disabled={captureInterfacesLoading}
                title={t("trdp.actions.refreshInterfaces")}
                aria-label={t("trdp.actions.refreshInterfaces")}
              >
                <Icon name="refresh" size="md" />
              </button>
            </div>
            {captureInterfacesError && <small className={styles.hint}>{captureInterfacesError}</small>}
          </div>

          <label className={`liquid-glass-toggle ${styles.toggle}`}>
            <input
              type="checkbox"
              checked={captureInterfaceBEditorEnabled}
              onChange={event => {
                setCaptureInterfaceBEditorEnabled(event.target.checked);
                if (!event.target.checked) {
                  patch({ capture_interfaces: { a: captureInterfaceA, b: null } });
                }
              }}
            />
            <div />
            <span>{t("trdp.form.captureLinkB")}</span>
          </label>

          {captureInterfaceBEditorEnabled && (
            <div className={styles.field}>
              <label className={styles.label}>{t("trdp.form.captureInterfaceB")}</label>
              <select
                className={`${styles.select} liquid-glass-input liquid-glass-select`}
                value={captureInterfaceB?.deviceName ?? ""}
                onChange={event => {
                  const selected = captureInterfaces.find(item => item.name === event.target.value);
                  patch({
                    capture_interfaces: {
                      a: captureInterfaceA,
                      b: selected ? captureInterfaceRef(selected) : null,
                    },
                  });
                }}
                disabled={captureInterfacesLoading || captureInterfaceA === null}
              >
                <option value="">{t("trdp.captureInterfaces.choose")}</option>
                {captureInterfaceB && !captureInterfaceBKnown && captureInterfaceB.deviceName !== captureInterfaceA?.deviceName && (
                  <option value={captureInterfaceB.deviceName}>{captureInterfaceB.displayName}</option>
                )}
                {captureInterfaces
                  .filter(item => item.name !== captureInterfaceA?.deviceName)
                  .map(item => (
                    <option key={item.name} value={item.name}>
                      {item.description ? `${item.description} — ${item.name}` : item.name}
                    </option>
                  ))}
              </select>
              {captureInterfaceA === null && (
                <small className={styles.hint}>{t("trdp.captureInterfaces.choose")}</small>
              )}
            </div>
          )}

          <label className={`liquid-glass-toggle ${styles.toggle}`}>
            <input
              type="checkbox"
              checked={captureFilterAuto}
              onChange={event => patch({ capture_filter_auto: event.target.checked })}
            />
            <div />
            <span>{t("trdp.form.autoFilter")}</span>
          </label>

          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.captureFilter")}</label>
            {captureFilterAuto ? (
              <code className={styles.filterPreview}>{captureFilterForPorts(pdPort, mdUdpPort, mdTcpPort)}</code>
            ) : (
              <input
                className={`${styles.input} liquid-glass-input`}
                value={captureFilter}
                onChange={event => patch({ capture_filter: event.target.value })}
              />
            )}
          </div>

          <details className={`${styles.details} liquid-glass-card`}>
            <summary className={styles.detailsSummary}><Icon name="chevron-right" size="xs" className={styles.detailsChevron} />{t("trdp.actions.advanced")}</summary>
            <div className={styles.detailsBody}>{portFields}</div>
          </details>
        </>
      )}
    </div>
  );
}
