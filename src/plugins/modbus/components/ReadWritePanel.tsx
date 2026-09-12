import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import styles from "../Modbus.module.css";
import {
  FUNCTION_LABELS,
  parseCoils,
  parseU16List,
  traditionalAddress,
  type BitReadArea,
  type ModbusRequest,
  type RegisterReadArea,
  type TransactionResult,
} from "../model";
import ResultCard from "./ResultCard";

interface Props {
  execute: (request: ModbusRequest) => Promise<TransactionResult>;
  connected: boolean;
}

interface ResultView {
  result: TransactionResult;
  functionCode: number;
  address: number;
  quantity: number;
}

const COMMON_FUNCTIONS = [1, 2, 3, 4, 5, 6, 15, 16, 22, 23] as const;

function singleRegister(text: string): number {
  const values = parseU16List(text);
  if (values.length !== 1) throw new Error("single register write requires exactly one 0..65535 value");
  return values[0];
}

function singleCoil(text: string): boolean {
  const values = parseCoils(text);
  if (values.length !== 1) throw new Error("single coil write requires exactly one 0/1/true/false value");
  return values[0];
}

function bitArea(fc: 1 | 2): BitReadArea {
  return fc === 1 ? "coils" : "discrete_inputs";
}

function registerArea(fc: 3 | 4): RegisterReadArea {
  return fc === 3 ? "holding_registers" : "input_registers";
}

export default function ReadWritePanel({ execute, connected }: Props) {
  const { t } = useTranslation();
  const [fc, setFc] = useState<number>(3);
  const [address, setAddress] = useState(0);
  const [quantity, setQuantity] = useState(1);
  const [value, setValue] = useState("0");
  const [andMask, setAndMask] = useState(0xffff);
  const [orMask, setOrMask] = useState(0);
  const [readAddress, setReadAddress] = useState(0);
  const [readQuantity, setReadQuantity] = useState(1);
  const [resultView, setResultView] = useState<ResultView | null>(null);
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
      let request: ModbusRequest;
      if (fc === 1 || fc === 2) {
        request = { kind: "read_bits", area: bitArea(fc), address, quantity };
      } else if (fc === 3 || fc === 4) {
        request = { kind: "read_registers", area: registerArea(fc), address, quantity };
      } else if (fc === 5) {
        request = { kind: "write_single_coil", address, value: singleCoil(value) };
      } else if (fc === 6) {
        request = { kind: "write_single_register", address, value: singleRegister(value) };
      } else if (fc === 15) {
        request = { kind: "write_multiple_coils", address, values: parseCoils(value) };
      } else if (fc === 16) {
        const values = parseU16List(value);
        if (values.length === 0) throw new Error("multiple register write requires at least one value");
        request = { kind: "write_multiple_registers", address, values };
      } else if (fc === 22) {
        request = { kind: "mask_write_register", address, and_mask: andMask, or_mask: orMask };
      } else {
        const values = parseU16List(value);
        if (values.length === 0) throw new Error("read/write multiple registers requires at least one write value");
        request = {
          kind: "read_write_multiple_registers",
          read_address: readAddress,
          read_quantity: readQuantity,
          write_address: address,
          values,
        };
      }
      const result = await execute(request);
      setResultView({
        result,
        functionCode: fc,
        address: fc === 23 ? readAddress : address,
        quantity: fc === 23 ? readQuantity : quantity,
      });
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.panelPage}>
      <section className={styles.workbenchSection}>
        <div className={styles.panelHeading}>
          <div>
            <strong>{t("modbus.rwRequest")}</strong>
            <span className={styles.hint}>{t("modbus.rwRequestHint")}</span>
          </div>
        </div>

        <div className={styles.grid}>
          <label className={styles.field}>
            <span className={styles.label}>{t("modbus.rwFunction")}</span>
            <select className="liquid-glass-input liquid-glass-select" value={fc} onChange={event => setFc(Number(event.target.value))} data-testid="tauterm-modbus-function">
              {COMMON_FUNCTIONS.map(code => <option key={code} value={code}>{FUNCTION_LABELS[code]}</option>)}
            </select>
          </label>
          {!isReadWrite && <label className={styles.field}>
            <span className={styles.label}>{t("modbus.rwProtocolAddress")}</span>
            <input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} />
            <span className={styles.hint}>{t("modbus.rwTraditionalReference", { reference: traditionalAddress(fc, address) })}</span>
          </label>}
          {isRead && <label className={styles.field}>
            <span className={styles.label}>{t("modbus.rwQuantity")}</span>
            <input className="liquid-glass-input" type="number" min={1} value={quantity} onChange={event => setQuantity(Number(event.target.value))} />
          </label>}
        </div>

        {isReadWrite && <div className={styles.grid}>
          <label className={styles.field}><span className={styles.label}>{t("modbus.rwReadAddress")}</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={readAddress} onChange={event => setReadAddress(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>{t("modbus.rwReadQuantity")}</span><input className="liquid-glass-input" type="number" min={1} value={readQuantity} onChange={event => setReadQuantity(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>{t("modbus.rwWriteAddress")}</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={address} onChange={event => setAddress(Number(event.target.value))} /></label>
        </div>}

        {isMask && <div className={styles.twoColumns}>
          <label className={styles.field}><span className={styles.label}>AND Mask</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={andMask} onChange={event => setAndMask(Number(event.target.value))} /></label>
          <label className={styles.field}><span className={styles.label}>OR Mask</span><input className="liquid-glass-input" type="number" min={0} max={65535} value={orMask} onChange={event => setOrMask(Number(event.target.value))} /></label>
        </div>}

        {!isRead && !isMask && <label className={styles.field}>
          <span className={styles.label}>{isMulti || isReadWrite ? t("modbus.rwValueList") : fc === 5 ? t("modbus.rwCoilValue") : t("modbus.rwValue")}</span>
          <textarea className={`${styles.textarea} liquid-glass-input`} value={value} onChange={event => setValue(event.target.value)} placeholder={fc === 15 ? "1 0 1 1" : "10 20 30 / 0x0010 0x0020"} />
        </label>}

        <div className={styles.actions}>
          <button className={`${styles.button} liquid-glass-button`} disabled={busy || !connected} onClick={() => void run()} data-testid="tauterm-modbus-execute">{busy ? t("modbus.executing") : t("modbus.execute")}</button>
          {!connected && <span className={styles.hint}>{t("modbus.connectToExecute")}</span>}
          {error && <span className={styles.error}>{error}</span>}
        </div>
      </section>

      <section className={styles.workbenchSection}>
        <div className={styles.panelHeading}>
          <div>
            <strong>{t("modbus.rwResult")}</strong>
            <span className={styles.hint}>{t("modbus.rwResultHint")}</span>
          </div>
        </div>
        {resultView ? <>
          <ReadValues result={resultView.result} functionCode={resultView.functionCode} address={resultView.address} quantity={resultView.quantity} />
          <ResultCard result={resultView.result} />
        </> : <div className={styles.emptyState}>{t("modbus.rwEmpty")}</div>}
      </section>
    </div>
  );
}

function ReadValues({ result, functionCode, address, quantity }: { result: TransactionResult; functionCode: number; address: number; quantity: number }) {
  const { t } = useTranslation();
  const rows = useMemo(() => {
    if (result.status !== "success" || !result.semantic_response) return [] as { address: number; value: string; hex: string }[];
    if (result.semantic_response.kind === "bits") {
      return result.semantic_response.values.slice(0, quantity).map((value, index) => ({
        address: address + index,
        value: value ? "1" : "0",
        hex: value ? "01" : "00",
      }));
    }
    if (result.semantic_response.kind === "registers") {
      return result.semantic_response.values.slice(0, quantity).map((value, index) => ({
        address: address + index,
        value: String(value),
        hex: `0x${value.toString(16).padStart(4, "0").toUpperCase()}`,
      }));
    }
    return [];
  }, [address, quantity, result]);

  if (rows.length === 0) return null;
  return <div className={styles.tableWrap}>
    <table className={styles.table}>
      <thead><tr><th>{t("modbus.columnProtocolAddress")}</th><th>{t("modbus.columnTraditionalReference")}</th><th>{t("modbus.columnValue")}</th><th>{t("modbus.columnHex")}</th></tr></thead>
      <tbody>{rows.map(row => <tr key={row.address}><td>{row.address}</td><td>{traditionalAddress(functionCode === 23 ? 3 : functionCode, row.address)}</td><td className={styles.mono}>{row.value}</td><td className={styles.mono}>{row.hex}</td></tr>)}</tbody>
    </table>
  </div>;
}
