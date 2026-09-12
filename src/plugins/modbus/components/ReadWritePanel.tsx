import { useState } from "react";
import styles from "../Modbus.module.css";
import {
  FUNCTION_LABELS,
  packCoils,
  parseU16List,
  traditionalAddress,
  type ModbusOperation,
  type TransactionResult,
} from "../model";
import ResultCard from "./ResultCard";

interface Props {
  execute: (request: ModbusOperation) => Promise<TransactionResult>;
  connected: boolean;
}

const COMMON_FUNCTIONS = [1, 2, 3, 4, 5, 6, 15, 16, 22, 23] as const;

export default function ReadWritePanel({ execute, connected }: Props) {
  const [fc, setFc] = useState<number>(3);
  const [address, setAddress] = useState(0);
  const [quantity, setQuantity] = useState(1);
  const [value, setValue] = useState("0");
  const [andMask, setAndMask] = useState(0xffff);
  const [orMask, setOrMask] = useState(0);
  const [readAddress, setReadAddress] = useState(0);
  const [readQuantity, setReadQuantity] = useState(1);
  const [result, setResult] = useState<TransactionResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const isRead = [1, 2, 3, 4].includes(fc);
  const isMulti = [15, 16].includes(fc);
  const isMask = fc === 22;
  const isReadWrite = fc === 23;

  const run = async () => {
    if (!connected) return;
    setBusy(true);
    setError("");
    try {
      let request: ModbusOperation;
      if (fc === 1 || fc === 2) {
        request = { kind: "read_bits", function: fc, address, quantity };
      } else if (fc === 3 || fc === 4) {
        request = { kind: "read_registers", function: fc, address, quantity };
      } else if (fc === 5 || fc === 6) {
        request = {
          kind: "write_single",
          function: fc,
          address,
          value: fc === 5 ? (Number(value) !== 0 ? 0xff00 : 0) : parseU16List(value)[0] ?? 0,
        };
      } else if (fc === 15) {
        const coils = packCoils(value);
        request = { kind: "write_multiple_coils", address, ...coils };
      } else if (fc === 16) {
        request = { kind: "write_multiple_registers", address, values: parseU16List(value) };
      } else if (fc === 22) {
        request = { kind: "mask_write_register", address, and_mask: andMask, or_mask: orMask };
      } else {
        request = {
          kind: "read_write_multiple_registers",
          read_address: readAddress,
          read_quantity: readQuantity,
          write_address: address,
          values: parseU16List(value),
        };
      }
      setResult(await execute(request));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.stack}>
      <section className={`${styles.card} liquid-glass-card`}>
        <div className={styles.panelHeading}>
          <div>
            <strong>请求</strong>
            <span className={styles.hint}>选择功能码并编辑本次事务参数。协议地址统一为 0-based。</span>
          </div>
        </div>

        <div className={styles.grid}>
          <label className={styles.field}>
            <span className={styles.label}>功能</span>
            <select className="liquid-glass-input liquid-glass-select" value={fc} onChange={event => setFc(Number(event.target.value))} data-testid="tauterm-modbus-function">
              {COMMON_FUNCTIONS.map(code => <option key={code} value={code}>{FUNCTION_LABELS[code]}</option>)}
            </select>
          </label>
          {!isReadWrite && <label className={styles.field}>
            <span className={styles.label}>协议地址 (0-based)</span>
            <input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} />
            <span className={styles.hint}>传统引用：{traditionalAddress(fc, address)}</span>
          </label>}
          {isRead && <label className={styles.field}>
            <span className={styles.label}>数量</span>
            <input className="liquid-glass-input" type="number" min={1} max={fc <= 2 ? 2000 : 125} value={quantity} onChange={event => setQuantity(Number(event.target.value))} />
          </label>}
        </div>

        {isReadWrite && <div className={styles.grid}>
          <label className={styles.field}><span className={styles.label}>读取地址</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={readAddress} onChange={event => setReadAddress(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>读取数量</span><input className="liquid-glass-input" type="number" min={1} max={125} value={readQuantity} onChange={event => setReadQuantity(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>写入地址</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} /></label>
        </div>}

        {isMask && <div className={styles.twoColumns}>
          <label className={styles.field}><span className={styles.label}>AND Mask</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={andMask} onChange={event => setAndMask(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>OR Mask</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={orMask} onChange={event => setOrMask(Number(event.target.value))} /></label>
        </div>}

        {!isRead && !isMask && <label className={styles.field}>
          <span className={styles.label}>{isMulti || isReadWrite ? "值列表" : fc === 5 ? "线圈值 (0/1)" : "值"}</span>
          <textarea className={`${styles.textarea} liquid-glass-input`} value={value} onChange={event => setValue(event.target.value)} placeholder={fc === 15 ? "1 0 1 1" : "10 20 30 / 0x0010 0x0020"} />
        </label>}

        <div className={styles.actions}>
          <button className={`${styles.button} liquid-glass-button`} disabled={busy || !connected} onClick={() => void run()} data-testid="tauterm-modbus-execute">{busy ? "执行中…" : "执行"}</button>
          {!connected && <span className={styles.hint}>连接会话后才能执行请求。</span>}
          {error && <span className={styles.error}>{error}</span>}
        </div>
      </section>

      <section className={`${styles.card} liquid-glass-card`}>
        <div className={styles.panelHeading}>
          <div>
            <strong>执行结果</strong>
            <span className={styles.hint}>显示当前请求的状态、延迟以及 TX / RX / PDU 原始数据。</span>
          </div>
        </div>
        {result ? <ResultCard result={result} /> : <div className={styles.emptyState}>尚未执行请求。完成一次事务后，结果会显示在这里。</div>}
      </section>
    </div>
  );
}
