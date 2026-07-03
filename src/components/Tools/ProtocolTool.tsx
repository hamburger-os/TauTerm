import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import RightSidebarPanel from "../RightSidebar/RightSidebarPanel";
import { parseHexString, crc16, numberToHex } from "../../utils/checksum";
import styles from "./ProtocolTool.module.css";

// ══════════════════════════════════════════════════════════════════
// 协议模板定义
// ══════════════════════════════════════════════════════════════════

type ProtocolTemplate = "at-response" | "modbus-rtu" | "modbus-ascii" | "custom";

interface ParsedField {
  name: string;
  offset: number;
  length: number;
  hexValue: string;
  parsedValue: string;
  /** i18n 插值参数（如 tools.fieldLine 需要 { n } 表示行号） */
  nameParams?: Record<string, unknown>;
}

interface ParseResult {
  fields: ParsedField[];
  checksumValid: boolean | null; // null = 无校验/无法验证
  checksumInfo: string;          // 用于无校验时的提示 key（由组件进行 i18n）
  checksumType?: "crc" | "lrc";
  checksumExpected?: string;
}

// ══════════════════════════════════════════════════════════════════
// Modbus RTU 帧解析
// ══════════════════════════════════════════════════════════════════

function parseModbusRTU(bytes: Uint8Array): ParseResult {
  const fields: ParsedField[] = [];

  // 设备地址 (1 byte)
  if (bytes.length >= 1) {
    fields.push({
      name: "tools.fieldDevAddr",
      offset: 0,
      length: 1,
      hexValue: bytes[0].toString(16).toUpperCase().padStart(2, "0"),
      parsedValue: String(bytes[0]),
    });
  }

  // 功能码 (1 byte)
  if (bytes.length >= 2) {
    const funcCode = bytes[1];
    const funcKeys: Record<number, string> = {
      0x01: "tools.funcReadCoils", 0x02: "tools.funcReadDiscreteInputs", 0x03: "tools.funcReadHoldingRegs",
      0x04: "tools.funcReadInputRegs", 0x05: "tools.funcWriteSingleCoil", 0x06: "tools.funcWriteSingleReg",
      0x0F: "tools.funcWriteMultiCoils", 0x10: "tools.funcWriteMultiRegs",
    };
    fields.push({
      name: "tools.fieldFuncCode",
      offset: 1,
      length: 1,
      hexValue: funcCode.toString(16).toUpperCase().padStart(2, "0"),
      parsedValue: funcKeys[funcCode] ?? `0x${funcCode.toString(16).toUpperCase()}`,
    });
  }

  // 数据区 (长度 = 总长 - 2 addr/func - 2 CRC)
  const dataLen = bytes.length - 4;
  if (dataLen > 0) {
    const dataBytes = bytes.slice(2, 2 + dataLen);
    fields.push({
      name: "tools.fieldData",
      offset: 2,
      length: dataLen,
      hexValue: Array.from(dataBytes, (b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" "),
      parsedValue: dataLen > 4 ? `(${dataLen} bytes)` : String(bytes[2]),
    });
  }

  // CRC16 (最后 2 bytes)
  let checksumValid: boolean | null = null;
  let checksumInfo = "";
  let checksumType: "crc" | "lrc" | undefined;
  let checksumExpected: string | undefined;
  if (bytes.length >= 4) {
    const crcOffset = bytes.length - 2;
    const crcBytes = bytes.slice(crcOffset, crcOffset + 2);
    const actualCrc = (crcBytes[1] << 8) | crcBytes[0]; // Modbus CRC 小端序
    const expectedCrc = crc16(bytes.slice(0, crcOffset), "CRC16-Modbus");
    fields.push({
      name: "tools.fieldCRC16",
      offset: crcOffset,
      length: 2,
      hexValue: Array.from(crcBytes, (b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" "),
      parsedValue: `0x${actualCrc.toString(16).toUpperCase().padStart(4, "0")}`,
    });
    checksumValid = actualCrc === expectedCrc;
    checksumType = "crc";
    checksumExpected = `0x${numberToHex(expectedCrc, 16)}`;
  }

  return { fields, checksumValid, checksumInfo, checksumType, checksumExpected };
}

// ══════════════════════════════════════════════════════════════════
// Modbus ASCII 帧解析
// ══════════════════════════════════════════════════════════════════

function parseModbusASCII(bytes: Uint8Array): ParseResult {
  // Modbus ASCII 帧格式: :Addr Func Data LRC CR LF
  // 先转字符串再解析
  const text = new TextDecoder().decode(bytes).trim();
  const fields: ParsedField[] = [];

  if (text.startsWith(":") && text.length >= 9) {
    fields.push({ name: "tools.fieldStartDelim", offset: 0, length: 1, hexValue: "3A", parsedValue: ":" });
    fields.push({ name: "tools.fieldAddress", offset: 1, length: 2, hexValue: text.slice(1, 3), parsedValue: String(parseInt(text.slice(1, 3), 16)) });
    fields.push({ name: "tools.fieldFuncCode", offset: 3, length: 2, hexValue: text.slice(3, 5), parsedValue: `0x${text.slice(3, 5)}` });

    const dataEnd = text.length - 4; // LRC(2) + CR(1) + LF(1)
    if (dataEnd > 5) {
      fields.push({ name: "tools.fieldData", offset: 5, length: dataEnd - 5, hexValue: text.slice(5, dataEnd), parsedValue: `(${dataEnd - 5} chars)` });
    }

    fields.push({
      name: "tools.fieldLRC",
      offset: dataEnd,
      length: 2,
      hexValue: text.slice(dataEnd, dataEnd + 2),
      parsedValue: text.slice(dataEnd, dataEnd + 2),
    });

    // LRC 验证 — 每次处理一个 HEX 字节（循环体中 i++ 与 for 的 i++ 形成步长 2）
    let lrc = 0;
    for (let i = 1; i < dataEnd; i++) {
      lrc += parseInt(text.slice(i, i + 2), 16);
      i++;
    }
    const expectedLrc = ((~lrc + 1) & 0xFF);
    const actualLrc = parseInt(text.slice(dataEnd, dataEnd + 2), 16);
    const valid = actualLrc === expectedLrc;

    return {
      fields,
      checksumValid: valid,
      checksumInfo: "",
      checksumType: "lrc",
      checksumExpected: numberToHex(expectedLrc, 8),
    };
  }

  return { fields, checksumValid: null, checksumInfo: "" };
}

// ══════════════════════════════════════════════════════════════════
// AT 指令响应解析
// ══════════════════════════════════════════════════════════════════

function parseATResponse(bytes: Uint8Array): ParseResult {
  const text = new TextDecoder().decode(bytes).trim();
  const fields: ParsedField[] = [];

  // 移除 \r\n
  const lines = text.split(/\r?\n/).filter((l) => l.trim().length > 0);

  if (lines.length > 0) {
    const firstLine = lines[0];
    if (firstLine.startsWith("+") || firstLine.startsWith("OK") || firstLine.startsWith("ERROR")) {
      fields.push({
        name: "tools.fieldRespType",
        offset: 0,
        length: firstLine.length,
        hexValue: Array.from(new TextEncoder().encode(firstLine), (b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" "),
        parsedValue: firstLine,
      });
    }

    // 解析 +XXX: 格式的响应
    const colonMatch = firstLine.match(/^\+(\w+):\s*(.+)/);
    if (colonMatch) {
      fields.push({
        name: `+${colonMatch[1]}`,
        offset: 0,
        length: firstLine.length,
        hexValue: "",
        parsedValue: colonMatch[2],
      });
    }
  }

  // 其他行
  if (lines.length > 1) {
    for (let i = 1; i < lines.length && i <= 5; i++) {
      fields.push({
        name: "tools.fieldLine",
        offset: 0,
        length: lines[i].length,
        hexValue: "",
        parsedValue: lines[i],
        nameParams: { n: i + 1 },
      });
    }
  }

  return { fields, checksumValid: null, checksumInfo: "tools.noChecksum" };
}

// ══════════════════════════════════════════════════════════════════
// 模板注册
// ══════════════════════════════════════════════════════════════════

const TEMPLATE_NAMES: Record<ProtocolTemplate, string> = {
  "at-response": "tools.protocolTemplateAT",
  "modbus-rtu": "tools.protocolTemplateModbusRTU",
  "modbus-ascii": "tools.protocolTemplateModbusASCII",
  "custom": "tools.protocolTemplateCustom",
};

const PARSERS: Record<ProtocolTemplate, (bytes: Uint8Array) => ParseResult> = {
  "at-response": parseATResponse,
  "modbus-rtu": parseModbusRTU,
  "modbus-ascii": parseModbusASCII,
  "custom": (bytes) => ({
    fields: [{
      name: "tools.fieldRawData",
      offset: 0,
      length: bytes.length,
      hexValue: Array.from(bytes, (b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" "),
      parsedValue: `(${bytes.length} bytes)`,
    }],
    checksumValid: null,
    checksumInfo: "tools.rawDataOnly",
  }),
};

// ══════════════════════════════════════════════════════════════════
// Component
// ══════════════════════════════════════════════════════════════════

export default function ProtocolTool() {
  const { t } = useTranslation();

  const [template, setTemplate] = useState<ProtocolTemplate>("modbus-rtu");
  const [hexInput, setHexInput] = useState("");

  const result = useMemo(() => {
    if (!hexInput.trim()) return null;
    const bytes = parseHexString(hexInput);
    if (bytes.length === 0) return null;
    return PARSERS[template](bytes);
  }, [hexInput, template]);

  return (
    <RightSidebarPanel title={t("tools.protocol") ?? "Protocol Parser"}>
      <div className={styles.container}>
        {/* 协议模板选择 */}
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={template}
          onChange={(e) => setTemplate(e.target.value as ProtocolTemplate)}
        >
          {Object.entries(TEMPLATE_NAMES).map(([key, labelKey]) => (
            <option key={key} value={key}>{t(labelKey)}</option>
          ))}
        </select>

        {/* HEX 帧输入 */}
        <textarea
          className={`${styles.input} liquid-glass-input liquid-glass-textarea`}
          value={hexInput}
          onChange={(e) => setHexInput(e.target.value)}
          placeholder={t("tools.protocolPlaceholder") ?? "Enter HEX frame, e.g.: 01 03 00 00 00 01 84 0A"}
          rows={3}
          spellCheck={false}
        />

        {/* 解析结果 */}
        {result && (
          <div className={styles.resultSection}>
            {/* 字段表格 */}
            {result.fields.length > 0 && (
              <table className={styles.table}>
                <thead>
                  <tr>
                    <th>{t("tools.protocolField") ?? "Field"}</th>
                    <th>{t("tools.protocolOffset") ?? "Offset"}</th>
                    <th>{t("tools.protocolValue") ?? "Value"}</th>
                  </tr>
                </thead>
                <tbody>
                  {result.fields.map((f, i) => (
                    <tr key={i}>
                      <td>
                        <code>
                          {f.name.startsWith("tools.")
                            ? t(f.name, f.nameParams ?? {})
                            : f.name}
                        </code>
                      </td>
                      <td>{f.offset > 0 ? f.offset : "-"}</td>
                      <td className={styles.fieldValue}>
                        {f.hexValue && <code className={styles.hexCode}>{f.hexValue}</code>}
                        {f.parsedValue && (
                          <span className={styles.parsedVal}>
                            {f.parsedValue.startsWith("tools.")
                              ? t(f.parsedValue)
                              : f.parsedValue}
                          </span>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            {/* 校验和验证 */}
            {result.checksumValid !== null && (
              <div className={`${styles.checksumResult} ${result.checksumValid ? styles.valid : styles.invalid}`}>
                {result.checksumValid
                  ? (result.checksumType === "lrc" ? t("tools.lrcValid") : t("tools.crcValid"))
                  : (result.checksumType === "lrc"
                    ? t("tools.lrcMismatch", { expected: result.checksumExpected ?? "" })
                    : t("tools.crcMismatch", { expected: result.checksumExpected ?? "" })
                  )
                }
              </div>
            )}
            {result.checksumValid === null && result.checksumInfo && (
              <div className={styles.checksumInfo}>{t(result.checksumInfo)}</div>
            )}
          </div>
        )}

        {!result && !hexInput.trim() && (
          <div className={styles.placeholder}>
            {t("tools.protocolHint") ?? "Select protocol and enter HEX frame"}
          </div>
        )}

        {!result && hexInput.trim() && (
          <div className={styles.parseError}>
            {t("tools.protocolParseError") ?? "Cannot parse HEX data"}
          </div>
        )}
      </div>
    </RightSidebarPanel>
  );
}
