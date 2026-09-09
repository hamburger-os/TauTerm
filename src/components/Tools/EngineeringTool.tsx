import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  diffBytes,
  serialTiming,
  subnetInfo,
  timestampInfo,
} from "../../utils/engineering";
import styles from "./EngineeringTool.module.css";

type Mode = "serial" | "timestamp" | "subnet" | "diff";

const MODES: Mode[] = ["serial", "timestamp", "subnet", "diff"];

function hexByte(value: number | undefined): string {
  return value === undefined
    ? "—"
    : value.toString(16).toUpperCase().padStart(2, "0");
}

export default function EngineeringTool() {
  const { t } = useTranslation();
  const [mode, setMode] = useState<Mode>("serial");

  const [baud, setBaud] = useState("115200");
  const [dataBits, setDataBits] = useState<5 | 6 | 7 | 8>(8);
  const [parity, setParity] = useState<"none" | "odd" | "even" | "mark" | "space">("none");
  const [stopBits, setStopBits] = useState<1 | 1.5 | 2>(1);
  const [byteCount, setByteCount] = useState("100");

  const [timestamp, setTimestamp] = useState("");
  const [timestampUnit, setTimestampUnit] = useState<
    "seconds" | "milliseconds" | "hex-seconds" | "hex-milliseconds"
  >("seconds");

  const [ip, setIp] = useState("192.168.1.100");
  const [prefix, setPrefix] = useState("24");

  const [left, setLeft] = useState("");
  const [right, setRight] = useState("");

  const serial = useMemo(
    () => serialTiming(
      Number(baud),
      dataBits,
      parity,
      stopBits,
      Number(byteCount),
    ),
    [baud, byteCount, dataBits, parity, stopBits],
  );

  const time = useMemo(
    () => timestamp.trim() ? timestampInfo(timestamp, timestampUnit) : null,
    [timestamp, timestampUnit],
  );

  const subnet = useMemo(
    () => ip.trim() && prefix.trim()
      ? subnetInfo(ip, Number(prefix))
      : null,
    [ip, prefix],
  );

  const diff = useMemo(
    () => left.trim() && right.trim() ? diffBytes(left, right) : null,
    [left, right],
  );

  return (
    <div className={styles.container}>
      <div className={styles.modeRow + " liquid-selector-strip"}>
        {MODES.map((item) => (
          <button
            key={item}
            type="button"
            className={"liquid-glass-button liquid-selector-button " + (mode === item ? "active" : "")}
            onClick={() => setMode(item)}
          >
            {t("tools.engineeringModes." + item)}
          </button>
        ))}
      </div>

      {mode === "serial" && (
        <div className={styles.section}>
          <div className={styles.formGrid}>
            <label>
              {t("tools.baudRate")}
              <input className="liquid-glass-input" value={baud} onChange={(event) => setBaud(event.target.value)} inputMode="numeric" />
            </label>
            <label>
              {t("tools.dataBits")}
              <select className="liquid-glass-input liquid-glass-select" value={dataBits} onChange={(event) => setDataBits(Number(event.target.value) as 5 | 6 | 7 | 8)}>
                {[5, 6, 7, 8].map((item) => <option key={item} value={item}>{item}</option>)}
              </select>
            </label>
            <label>
              {t("tools.parity")}
              <select className="liquid-glass-input liquid-glass-select" value={parity} onChange={(event) => setParity(event.target.value as typeof parity)}>
                {(["none", "odd", "even", "mark", "space"] as const).map((item) => (
                  <option key={item} value={item}>{t("tools.parityValues." + item)}</option>
                ))}
              </select>
            </label>
            <label>
              {t("tools.stopBits")}
              <select className="liquid-glass-input liquid-glass-select" value={stopBits} onChange={(event) => setStopBits(Number(event.target.value) as 1 | 1.5 | 2)}>
                {[1, 1.5, 2].map((item) => <option key={item} value={item}>{item}</option>)}
              </select>
            </label>
            <label>
              {t("tools.byteCount")}
              <input className="liquid-glass-input" value={byteCount} onChange={(event) => setByteCount(event.target.value)} inputMode="numeric" />
            </label>
          </div>

          {serial.ok ? (
            <div className={styles.resultGrid}>
              <span>{t("tools.bitsPerFrame")}</span><code>{serial.value.bitsPerFrame}</code>
              <span>{t("tools.timePerByte")}</span><code>{serial.value.microsecondsPerByte.toFixed(3)} μs</code>
              <span>{t("tools.totalTime")}</span><code>{serial.value.millisecondsTotal.toFixed(3)} ms</code>
            </div>
          ) : (
            <div className={styles.error}>{t("tools.errors." + serial.error.code)}</div>
          )}
        </div>
      )}

      {mode === "timestamp" && (
        <div className={styles.section}>
          <select
            className="liquid-glass-input liquid-glass-select"
            value={timestampUnit}
            onChange={(event) => setTimestampUnit(event.target.value as typeof timestampUnit)}
          >
            {(["seconds", "milliseconds", "hex-seconds", "hex-milliseconds"] as const).map((item) => (
              <option key={item} value={item}>{t("tools.timestampUnits." + item)}</option>
            ))}
          </select>
          <input
            className="liquid-glass-input"
            value={timestamp}
            onChange={(event) => setTimestamp(event.target.value)}
            placeholder={t("tools.timestampPlaceholder")}
            spellCheck={false}
          />
          {time?.ok && (
            <div className={styles.resultGrid}>
              <span>ISO</span><code>{time.value.iso}</code>
              <span>UTC</span><code>{time.value.utc}</code>
              <span>{t("tools.localTime")}</span><code>{time.value.local}</code>
            </div>
          )}
          {time && !time.ok && (
            <div className={styles.error}>{t("tools.errors." + time.error.code)}</div>
          )}
        </div>
      )}

      {mode === "subnet" && (
        <div className={styles.section}>
          <div className={styles.inlineFields}>
            <input className="liquid-glass-input" value={ip} onChange={(event) => setIp(event.target.value)} placeholder="192.168.1.100" />
            <span>/</span>
            <input className={styles.prefixInput + " liquid-glass-input"} value={prefix} onChange={(event) => setPrefix(event.target.value)} inputMode="numeric" />
          </div>
          {subnet?.ok && (
            <div className={styles.resultGrid}>
              <span>{t("tools.netmask")}</span><code>{subnet.value.mask}</code>
              <span>{t("tools.networkAddress")}</span><code>{subnet.value.network}</code>
              <span>{t("tools.broadcastAddress")}</span><code>{subnet.value.broadcast}</code>
              <span>{t("tools.firstHost")}</span><code>{subnet.value.firstHost}</code>
              <span>{t("tools.lastHost")}</span><code>{subnet.value.lastHost}</code>
              <span>{t("tools.hostCount")}</span><code>{subnet.value.hostCount}</code>
            </div>
          )}
          {subnet && !subnet.ok && (
            <div className={styles.error}>{t("tools.errors." + subnet.error.code)}</div>
          )}
        </div>
      )}

      {mode === "diff" && (
        <div className={styles.section}>
          <textarea
            className="liquid-glass-input liquid-glass-textarea"
            value={left}
            onChange={(event) => setLeft(event.target.value)}
            placeholder={t("tools.byteDiffLeft")}
            rows={3}
            spellCheck={false}
          />
          <textarea
            className="liquid-glass-input liquid-glass-textarea"
            value={right}
            onChange={(event) => setRight(event.target.value)}
            placeholder={t("tools.byteDiffRight")}
            rows={3}
            spellCheck={false}
          />
          {diff?.ok && (
            <>
              <div className={styles.diffSummary}>
                {t("tools.byteDiffSummary", {
                  left: diff.value.leftLength,
                  right: diff.value.rightLength,
                  count: diff.value.differences.length,
                })}
              </div>
              {diff.value.differences.length > 0 && (
                <div className={styles.tableWrap}>
                  <table className={styles.table}>
                    <thead>
                      <tr>
                        <th>{t("tools.offset")}</th>
                        <th>A</th>
                        <th>B</th>
                        <th>XOR</th>
                      </tr>
                    </thead>
                    <tbody>
                      {diff.value.differences.map((entry) => (
                        <tr key={entry.offset}>
                          <td>{entry.offset}</td>
                          <td><code>{hexByte(entry.left)}</code></td>
                          <td><code>{hexByte(entry.right)}</code></td>
                          <td><code>{hexByte(entry.xor)}</code></td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
          {diff && !diff.ok && (
            <div className={styles.error}>
              {t("tools.errors." + diff.error.code, {
                detail: diff.error.detail ?? "",
                defaultValue: diff.error.code,
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
