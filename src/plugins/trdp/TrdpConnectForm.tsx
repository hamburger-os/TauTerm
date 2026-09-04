import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import Icon from "../../components/common/Icon";
import styles from "./TrdpConnectForm.module.css";

type CaptureInterface = {
  name: string;
  description: string;
};

const STANDARD_CAPTURE_FILTER = "udp port 17224 or udp port 17225 or tcp port 17225";

function captureFilterForPorts(pdPort: number, mdUdpPort: number, mdTcpPort: number) {
  return `udp port ${pdPort} or udp port ${mdUdpPort} or tcp port ${mdTcpPort}`;
}

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
  const [captureInterfacesError, setCaptureInterfacesError] = useState("");

  useEffect(() => {
    if (mode !== "monitor") return;
    let cancelled = false;
    setCaptureInterfacesLoading(true);
    setCaptureInterfacesError("");
    void invoke<CaptureInterface[]>("trdp_capture_interfaces")
      .then(items => {
        if (!cancelled) setCaptureInterfaces(items);
      })
      .catch(error => {
        if (!cancelled) {
          setCaptureInterfaces([]);
          setCaptureInterfacesError(String(error));
        }
      })
      .finally(() => {
        if (!cancelled) setCaptureInterfacesLoading(false);
      });
    return () => { cancelled = true; };
  }, [mode]);
  const patch = (next: Record<string, unknown>) => onChange({
    mode: "node",
    link_a_ip: "0.0.0.0",
    link_b_enabled: false,
    link_b_ip: "0.0.0.0",
    pd_port: 17224,
    md_udp_port: 17225,
    md_tcp_port: 17225,
    capture_interface: "",
    capture_interface_b_enabled: false,
    capture_interface_b: "",
    capture_filter_auto: bool(
      params,
      "capture_filter_auto",
      str(params, "capture_filter", STANDARD_CAPTURE_FILTER) === STANDARD_CAPTURE_FILTER,
    ),
    capture_filter: STANDARD_CAPTURE_FILTER,
    ...params,
    ...next,
  });

  const captureFilterValue = str(params, "capture_filter", STANDARD_CAPTURE_FILTER);
  const captureFilterAuto = bool(params, "capture_filter_auto", captureFilterValue === STANDARD_CAPTURE_FILTER);
  const effectiveCaptureFilter = captureFilterForPorts(
    num(params, "pd_port", 17224),
    num(params, "md_udp_port", 17225),
    num(params, "md_tcp_port", 17225),
  );

  async function chooseXml() {
    const path = await open({
      multiple: false,
      filters: [{ name: "TRDP XML", extensions: ["xml"] }],
    });
    if (typeof path === "string") patch({ xml_path: path });
  }

  const portFields = (
    <div className={styles.ports}>
      <div className={styles.field}>
        <label className={styles.label}>{t("trdp.form.pdUdpPort")}</label>
        <input
          className={`${styles.numberInput} liquid-glass-input`}
          type="number"
          min={1}
          max={65535}
          value={num(params, "pd_port", 17224)}
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
          value={num(params, "md_udp_port", 17225)}
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
          value={num(params, "md_tcp_port", 17225)}
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
            <summary>{t("trdp.actions.advanced")}</summary>
            <div className={styles.detailsBody}>{portFields}</div>
          </details>
        </>
      ) : (
        <>
          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.captureInterfaceA")}</label>
            <select
              className={`${styles.select} liquid-glass-input liquid-glass-select`}
              value={str(params, "capture_interface")}
              onChange={e => patch({ capture_interface: e.target.value })}
              disabled={captureInterfacesLoading}
            >
              <option value="">
                {captureInterfacesLoading
                  ? t("trdp.captureInterfaces.loading")
                  : t("trdp.captureInterfaces.choose")}
              </option>
              {captureInterfaces.map(item => (
                <option key={item.name} value={item.name}>
                  {item.description ? `${item.description} — ${item.name}` : item.name}
                </option>
              ))}
            </select>
            {captureInterfacesError && <small className={styles.hint}>{t("trdp.captureInterfaces.error")}: {captureInterfacesError}</small>}
            {!captureInterfacesLoading && !captureInterfacesError && captureInterfaces.length === 0 && (
              <small className={styles.hint}>{t("trdp.captureInterfaces.empty")}</small>
            )}
          </div>

          <label className={`liquid-glass-toggle ${styles.toggle}`}>
            <input
              type="checkbox"
              checked={bool(params, "capture_interface_b_enabled")}
              onChange={e => patch({ capture_interface_b_enabled: e.target.checked })}
            />
            <div />
            <span>{t("trdp.form.captureLinkB")}</span>
          </label>

          {bool(params, "capture_interface_b_enabled") && (
            <div className={styles.field}>
              <label className={styles.label}>{t("trdp.form.captureInterfaceB")}</label>
              <select
                className={`${styles.select} liquid-glass-input liquid-glass-select`}
                value={str(params, "capture_interface_b")}
                onChange={e => patch({ capture_interface_b: e.target.value })}
                disabled={captureInterfacesLoading}
              >
                <option value="">
                  {captureInterfacesLoading
                    ? t("trdp.captureInterfaces.loading")
                    : t("trdp.captureInterfaces.choose")}
                </option>
                {captureInterfaces
                  .filter(item => item.name !== str(params, "capture_interface"))
                  .map(item => (
                    <option key={item.name} value={item.name}>
                      {item.description ? `${item.description} — ${item.name}` : item.name}
                    </option>
                  ))}
              </select>
            </div>
          )}

          <label className={`liquid-glass-toggle ${styles.toggle}`}>
            <input
              type="checkbox"
              checked={captureFilterAuto}
              onChange={e => patch(e.target.checked
                ? { capture_filter_auto: true }
                : { capture_filter_auto: false, capture_filter: effectiveCaptureFilter })}
            />
            <div />
            <span>{t("trdp.form.autoFilter")}</span>
          </label>

          <div className={styles.field}>
            <label className={styles.label}>{t("trdp.form.captureFilter")}</label>
            {captureFilterAuto
              ? <code className={styles.filterPreview}>{effectiveCaptureFilter}</code>
              : (
                <input
                  className={`${styles.input} liquid-glass-input`}
                  value={captureFilterValue}
                  onChange={e => patch({ capture_filter: e.target.value })}
                />
              )}
            <small className={styles.hint}>
              {captureFilterAuto
                ? t("trdp.form.autoFilterHint")
                : t("trdp.form.customFilterHint")}
            </small>
          </div>

          <details className={`${styles.details} liquid-glass-card`}>
            <summary>{t("trdp.actions.advanced")}</summary>
            <div className={styles.detailsBody}>{portFields}</div>
          </details>

          <p className={styles.note}>
            {t("trdp.form.monitorNote")}
          </p>
        </>
      )}
    </div>
  );
}
