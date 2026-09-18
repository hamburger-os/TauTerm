import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import Icon from "../../components/common/Icon";
import {
  compactRttProbeName,
  normalizeRttParams,
  type RttProbeInfo,
} from "./model";
import styles from "./RttConnectForm.module.css";

function str(params: Record<string, unknown>, key: string, fallback = ""): string {
  const value = params[key];
  return typeof value === "string" ? value : fallback;
}

function num(params: Record<string, unknown>, key: string, fallback: number): number {
  const value = params[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function optionalPositiveNumber(params: Record<string, unknown>, key: string): number | "" {
  const value = params[key];
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : "";
}

export default function RttConnectForm({ params, onChange, disabled }: ConnectFormProps) {
  const { t } = useTranslation();
  const normalized = normalizeRttParams(params);
  const backend = str(normalized, "backend", "probe_rs");
  const [probes, setProbes] = useState<RttProbeInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);

  const patch = (next: Record<string, unknown>) => onChange({ ...normalized, ...next });

  async function refreshProbes() {
    setLoading(true);
    setProbeError(null);
    try {
      const items = await invoke<RttProbeInfo[]>("rtt_discover_probes");
      setProbes(items);
      if (items.length === 0) setProbeError(t("rtt.noProbes"));
    } catch (cause) {
      setProbes([]);
      setProbeError(`${t("rtt.discoverFailed")}: ${String(cause)}`);
    } finally {
      setLoading(false);
    }
  }

  async function chooseFirmware() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{
        name: t("rtt.firmwareArtifact"),
        extensions: ["elf", "axf", "out"],
      }],
    });
    if (typeof selected === "string") patch({ firmware_path: selected });
  }

  useEffect(() => {
    if (backend === "probe_rs") void refreshProbes();
  }, [backend]);

  const selector = str(normalized, "probe_selector");
  const locator = str(normalized, "locator_mode", "auto");
  const selectorKnown = probes.some(item => item.selector === selector);

  return (
    <div className={styles.root}>
      <div className={styles.field}>
        <label>{t("rtt.backend")}</label>
        <select
          className={`liquid-glass-input liquid-glass-select ${styles.control}`}
          value={backend}
          disabled={disabled}
          onChange={event => patch({ backend: event.target.value })}
        >
          <option value="probe_rs">{t("rtt.backendProbeRs")}</option>
          <option value="jlink_existing">{t("rtt.backendJlinkExisting")}</option>
        </select>
      </div>

      {backend === "probe_rs" ? (
        <>
          <div className={styles.field}>
            <label>{t("rtt.probe")}</label>
            <div className={styles.inlineField}>
              <select
                className={`liquid-glass-input liquid-glass-select ${styles.control}`}
                value={selector}
                disabled={disabled || loading}
                onChange={event => {
                  const nextSelector = event.target.value;
                  const selectedProbe = probes.find(item => item.selector === nextSelector);
                  patch({
                    probe_selector: nextSelector,
                    probe_name: selectedProbe ? compactRttProbeName(selectedProbe) : "",
                  });
                }}
              >
                <option value="">{t("rtt.probeAuto")}</option>
                {selector && !selectorKnown && <option value={selector}>{selector}</option>}
                {probes.map(item => (
                  <option key={item.selector} value={item.selector}>{item.display_name}</option>
                ))}
              </select>
              <button
                type="button"
                className={`liquid-glass-button ${styles.iconButton}`}
                disabled={disabled || loading}
                onClick={() => void refreshProbes()}
                aria-label={t("rtt.refresh")}
                title={t("rtt.refresh")}
              >
                <Icon name="refresh" size="md" />
              </button>
            </div>
            {probeError && <small className={styles.error}>{probeError}</small>}
          </div>

          <div className={styles.grid2}>
            <div className={styles.field}>
              <label>{t("rtt.target")}</label>
              <input
                className={`liquid-glass-input ${styles.control}`}
                value={str(normalized, "target")}
                disabled={disabled}
                placeholder={t("rtt.targetPlaceholder")}
                onChange={event => patch({ target: event.target.value })}
              />
            </div>
            <div className={styles.field}>
              <label>{t("rtt.wireProtocol")}</label>
              <select
                className={`liquid-glass-input liquid-glass-select ${styles.control}`}
                value={str(normalized, "wire_protocol", "swd")}
                disabled={disabled}
                onChange={event => patch({ wire_protocol: event.target.value })}
              >
                <option value="swd">SWD</option>
                <option value="jtag">JTAG</option>
              </select>
            </div>
          </div>

          <div className={styles.field}>
            <label>{t("rtt.firmwareArtifact")}</label>
            <div className={styles.inlineField}>
              <input
                className={`liquid-glass-input ${styles.control} ${styles.pathControl}`}
                value={str(normalized, "firmware_path")}
                disabled={disabled}
                placeholder={t("rtt.firmwarePlaceholder")}
                onChange={event => patch({ firmware_path: event.target.value })}
              />
              <button
                type="button"
                className={`liquid-glass-button ${styles.iconButton}`}
                disabled={disabled}
                onClick={() => void chooseFirmware()}
                aria-label={t("rtt.browseFirmware")}
                title={t("rtt.browseFirmware")}
              >
                <Icon name="folder" size="md" />
              </button>
            </div>
            <small>{t("rtt.firmwareHint")}</small>
          </div>

          <div className={styles.field}>
            <label>{t("rtt.locator")}</label>
            <select
              className={`liquid-glass-input liquid-glass-select ${styles.control}`}
              value={locator}
              disabled={disabled}
              onChange={event => patch({ locator_mode: event.target.value })}
            >
              <option value="auto">{t("rtt.locatorAuto")}</option>
              <option value="exact">{t("rtt.locatorExact")}</option>
              <option value="ranges">{t("rtt.locatorRanges")}</option>
            </select>
          </div>

          {locator === "exact" && (
            <div className={styles.field}>
              <label>{t("rtt.address")}</label>
              <input
                className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                value={str(normalized, "control_block_address")}
                disabled={disabled}
                placeholder="0x20000000"
                onChange={event => patch({ control_block_address: event.target.value })}
              />
            </div>
          )}

          {locator === "ranges" && (
            <div className={styles.field}>
              <label>{t("rtt.ranges")}</label>
              <input
                className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                value={str(normalized, "scan_ranges")}
                disabled={disabled}
                placeholder="0x20000000-0x20020000"
                onChange={event => patch({ scan_ranges: event.target.value })}
              />
              <small>{t("rtt.rangesHint")}</small>
            </div>
          )}

          <details className={`${styles.advanced} liquid-glass-card`}>
            <summary className={styles.advancedSummary}>
              <Icon name="chevron-right" size="xs" className={styles.advancedChevron} />
              {t("rtt.advanced")}
            </summary>
            <div className={styles.advancedBody}>
              <div className={styles.grid2}>
                <div className={styles.field}>
                  <label>{t("rtt.speed")}</label>
                  <input
                    className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                    type="number"
                    min={1}
                    max={50000}
                    value={optionalPositiveNumber(normalized, "speed_khz")}
                    disabled={disabled}
                    onChange={event => patch({
                      speed_khz: event.target.value === "" ? null : Number(event.target.value),
                    })}
                  />
                  <small>{t("rtt.speedAuto")}</small>
                </div>
                <div className={styles.field}>
                  <label>{t("rtt.core")}</label>
                  <input
                    className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                    type="number"
                    min={0}
                    max={31}
                    value={num(normalized, "core_index", 0)}
                    disabled={disabled}
                    onChange={event => patch({ core_index: Number(event.target.value) })}
                  />
                </div>
              </div>
            </div>
          </details>
        </>
      ) : (
        <>
          <div className={styles.grid2}>
            <div className={styles.field}>
              <label>{t("rtt.jlinkPort")}</label>
              <input
                className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                type="number"
                min={1}
                max={65535}
                value={num(normalized, "jlink_port", 19021)}
                disabled={disabled}
                onChange={event => patch({ jlink_port: Number(event.target.value) })}
              />
            </div>
            <div className={styles.field}>
              <label>{t("rtt.jlinkChannels")}</label>
              <input
                className={`liquid-glass-input ${styles.control} ${styles.numberControl}`}
                value={str(normalized, "jlink_channels", "0")}
                disabled={disabled}
                placeholder="0,1,2"
                onChange={event => patch({ jlink_channels: event.target.value })}
              />
            </div>
          </div>
          <small className={styles.hint}>{t("rtt.jlinkHint")}</small>
        </>
      )}
    </div>
  );
}
