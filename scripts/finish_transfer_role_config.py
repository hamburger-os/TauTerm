from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    if text.count(old) != 1:
        raise RuntimeError(f"{path}: expected exactly one occurrence of {old[:80]!r}, found {text.count(old)}")
    write(path, text.replace(old, new, 1))


# 1. Finish ZMODEM role-aware CRC negotiation migration.
zpath = "src-tauri/src/transfer/zmodem.rs"
z = read(zpath)
if "use crate::transfer::config::{ZModemCrcPolicy, ZModemReceiveCrcCapability};" not in z:
    z = z.replace(
        "use crate::transfer::crc::{crc16_ccitt, crc32_verify, crc32_zmodem};",
        "use crate::transfer::config::{ZModemCrcPolicy, ZModemReceiveCrcCapability};\nuse crate::transfer::crc::{crc16_ccitt, crc32_verify, crc32_zmodem};",
        1,
    )
needle = "    fs::create_dir_all(download_dir)?;\n\n    let mut current_file"
if needle in z:
    z = z.replace(
        needle,
        "    fs::create_dir_all(download_dir)?;\n    let use_crc32 = matches!(crc_capability, ZModemReceiveCrcCapability::Auto);\n\n    let mut current_file",
        1,
    )
old_test = '''    #[test]\n    fn test_zmodem_default() {\n        let z = ZModem::default();\n        assert!(z.use_crc32);\n        assert_eq!(z.max_block_size, 8192);\n        assert_eq!(z.window_size, 1);\n    }'''
new_test = '''    #[test]\n    fn sender_role_keeps_crc_policy_and_block_limit() {\n        let z = ZModem::sender(ZModemCrcPolicy::Crc32Required, 4096);\n        assert!(matches!(\n            z.role,\n            ZModemRole::Sender {\n                crc_policy: ZModemCrcPolicy::Crc32Required,\n                max_block_size: 4096\n            }\n        ));\n    }\n\n    #[test]\n    fn receiver_role_keeps_crc_capability() {\n        let z = ZModem::receiver(ZModemReceiveCrcCapability::Crc16Only);\n        assert!(matches!(\n            z.role,\n            ZModemRole::Receiver {\n                crc_capability: ZModemReceiveCrcCapability::Crc16Only\n            }\n        ));\n    }'''
if old_test not in z:
    raise RuntimeError("zmodem old default test not found")
z = z.replace(old_test, new_test, 1)
write(zpath, z)

# 2. Frontend transfer model mirrors the backend role-aware tagged unions.
tpath = "src/types/transfer.ts"
t = read(tpath)
start = t.index("// ── Protocol Config Interfaces")
end = t.index("// ── Transfer Events", start)
contract = '''// ── Protocol Config Interfaces ────────────────────────────\n\nexport type XmodemReceiveCheckMode = "auto" | "crc16" | "checksum";\nexport type ZmodemCrcPolicy = "auto" | "crc16" | "crc32-required";\nexport type ZmodemReceiveCrcCapability = "auto" | "crc16-only";\nexport type ModemBlockSize = 128 | 1024;\nexport type ZmodemMaxBlockSize = 1024 | 2048 | 4096 | 8192;\n\n/** YMODEM 的块大小属于发送方；接收方按标准流程主动请求 CRC16。 */\nexport interface YmodemTransferConfig {\n  protocol: "ymodem";\n  send: { blockSize: ModemBlockSize };\n}\n\n/** XMODEM 将发送块大小与接收校验请求分开建模。 */\nexport interface XmodemTransferConfig {\n  protocol: "xmodem";\n  send: { blockSize: ModemBlockSize };\n  receive: { checkMode: XmodemReceiveCheckMode };\n}\n\n/** ZMODEM 发送策略与接收方宣告的 CRC 能力分别建模。 */\nexport interface ZmodemTransferConfig {\n  protocol: "zmodem";\n  send: {\n    crcPolicy: ZmodemCrcPolicy;\n    maxBlockSize: ZmodemMaxBlockSize;\n  };\n  receive: { crcCapability: ZmodemReceiveCrcCapability };\n}\n\n/** SFTP 传输配置（文件管理器拥有真实远端目录状态）。 */\nexport interface SftpTransferConfig {\n  protocol: "sftp";\n  remotePath: string;\n}\n\nexport type TransferConfig =\n  | YmodemTransferConfig\n  | XmodemTransferConfig\n  | ZmodemTransferConfig\n  | SftpTransferConfig;\n\n// ── Protocol Registry ─────────────────────────────────────\n\nexport interface ProtocolMeta {\n  type: ProtocolType;\n  i18nKey: string;\n  icon: IconName;\n  defaultConfig: TransferConfig;\n}\n\nexport const PROTOCOL_REGISTRY: Record<ProtocolType, ProtocolMeta> = {\n  ymodem: {\n    type: "ymodem",\n    i18nKey: "transfer.protocols.ymodem.name",\n    icon: "package",\n    defaultConfig: {\n      protocol: "ymodem",\n      send: { blockSize: 1024 },\n    },\n  },\n  xmodem: {\n    type: "xmodem",\n    i18nKey: "transfer.protocols.xmodem.name",\n    icon: "package",\n    defaultConfig: {\n      protocol: "xmodem",\n      send: { blockSize: 128 },\n      receive: { checkMode: "auto" },\n    },\n  },\n  zmodem: {\n    type: "zmodem",\n    i18nKey: "transfer.protocols.zmodem.name",\n    icon: "package",\n    defaultConfig: {\n      protocol: "zmodem",\n      send: { crcPolicy: "auto", maxBlockSize: 8192 },\n      receive: { crcCapability: "auto" },\n    },\n  },\n  sftp: {\n    type: "sftp",\n    i18nKey: "transfer.protocols.sftp.name",\n    icon: "folder",\n    defaultConfig: {\n      protocol: "sftp",\n      remotePath: "/",\n    },\n  },\n};\n\n// ── Transfer Commands ─────────────────────────────────────\n\nexport type SendProtocolOptions =\n  | { protocol: "ymodem"; blockSize: ModemBlockSize }\n  | { protocol: "xmodem"; blockSize: ModemBlockSize }\n  | { protocol: "zmodem"; crcPolicy: ZmodemCrcPolicy; maxBlockSize: ZmodemMaxBlockSize }\n  | { protocol: "sftp" };\n\nexport type ReceiveProtocolOptions =\n  | { protocol: "ymodem" }\n  | { protocol: "xmodem"; checkMode: XmodemReceiveCheckMode }\n  | { protocol: "zmodem"; crcCapability: ZmodemReceiveCrcCapability }\n  | { protocol: "sftp" };\n\n/** 前端 → 后端发送命令：协议专有设置只存在于 role-aware protocolOptions。 */\nexport interface FileTransferSendRequest {\n  sessionId: string;\n  protocolOptions: SendProtocolOptions;\n  filePaths: string[];\n  remoteDir?: string;\n  overwritePolicy?: OverwritePolicy;\n}\n\n/** 前端 → 后端接收命令：不会复用发送方配置。 */\nexport interface FileTransferReceiveRequest {\n  sessionId: string;\n  protocolOptions: ReceiveProtocolOptions;\n  downloadDir: string;\n  remotePaths: string[];\n  destinationPaths?: string[];\n  overwritePolicy?: OverwritePolicy;\n}\n\n'''
write(tpath, t[:start] + contract + t[end:])

# 3. Direction-specific forms share the existing theme primitives.
write(
    "src/components/FileTransfer/protocol-config/forms/YmodemConfigForm.tsx",
    '''import { useTranslation } from "react-i18next";\nimport type { YmodemTransferConfig } from "../../../../types/transfer";\nimport styles from "./shared/ProtocolOptionForm.module.css";\n\ninterface YmodemConfigFormProps {\n  config: YmodemTransferConfig;\n  onChange: (config: YmodemTransferConfig) => void;\n}\n\n/** YMODEM 仅暴露发送方可控制的数据块大小；接收方固定按标准 CRC16 流程工作。 */\nexport default function YmodemConfigForm({ config, onChange }: YmodemConfigFormProps) {\n  const { t } = useTranslation();\n  return (\n    <div className={styles.form}>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configSendSettings")} · {t("transfer.configBlockSize")}\n        </label>\n        <div className={styles.btnRow}>\n          {([1024, 128] as const).map((blockSize) => (\n            <button\n              key={blockSize}\n              className={`${styles.optionBtn} liquid-glass-button ${config.send.blockSize === blockSize ? "active" : ""}`}\n              onClick={() => onChange({ ...config, send: { blockSize } })}\n            >\n              {blockSize === 1024 ? t("transfer.configBlockSize1K") : t("transfer.configBlockSize128")}\n            </button>\n          ))}\n        </div>\n      </div>\n    </div>\n  );\n}\n''',
)

write(
    "src/components/FileTransfer/protocol-config/forms/XmodemConfigForm.tsx",
    '''import { useTranslation } from "react-i18next";\nimport type { XmodemReceiveCheckMode, XmodemTransferConfig } from "../../../../types/transfer";\nimport styles from "./shared/ProtocolOptionForm.module.css";\n\ninterface XmodemConfigFormProps {\n  config: XmodemTransferConfig;\n  onChange: (config: XmodemTransferConfig) => void;\n}\n\nconst receiveModes: readonly XmodemReceiveCheckMode[] = ["auto", "crc16", "checksum"];\n\nexport default function XmodemConfigForm({ config, onChange }: XmodemConfigFormProps) {\n  const { t } = useTranslation();\n  return (\n    <div className={styles.form}>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configSendSettings")} · {t("transfer.configBlockSize")}\n        </label>\n        <div className={styles.btnRow}>\n          {([128, 1024] as const).map((blockSize) => (\n            <button\n              key={blockSize}\n              className={`${styles.optionBtn} liquid-glass-button ${config.send.blockSize === blockSize ? "active" : ""}`}\n              onClick={() => onChange({ ...config, send: { blockSize } })}\n            >\n              {blockSize === 1024 ? t("transfer.configBlockSize1K") : t("transfer.configBlockSize128")}\n            </button>\n          ))}\n        </div>\n      </div>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configReceiveSettings")} · {t("transfer.configChecksumRequest")}\n        </label>\n        <div className={styles.btnRow}>\n          {receiveModes.map((checkMode) => (\n            <button\n              key={checkMode}\n              className={`${styles.optionBtn} liquid-glass-button ${config.receive.checkMode === checkMode ? "active" : ""}`}\n              onClick={() => onChange({ ...config, receive: { checkMode } })}\n            >\n              {t(`transfer.configMode.${checkMode}`)}\n            </button>\n          ))}\n        </div>\n      </div>\n    </div>\n  );\n}\n''',
)

write(
    "src/components/FileTransfer/protocol-config/forms/ZmodemConfigForm.tsx",
    '''import { useTranslation } from "react-i18next";\nimport type {\n  ZmodemCrcPolicy,\n  ZmodemMaxBlockSize,\n  ZmodemReceiveCrcCapability,\n  ZmodemTransferConfig,\n} from "../../../../types/transfer";\nimport styles from "./shared/ProtocolOptionForm.module.css";\n\ninterface ZmodemConfigFormProps {\n  config: ZmodemTransferConfig;\n  onChange: (config: ZmodemTransferConfig) => void;\n}\n\nconst crcPolicies: readonly ZmodemCrcPolicy[] = ["auto", "crc16", "crc32-required"];\nconst crcCapabilities: readonly ZmodemReceiveCrcCapability[] = ["auto", "crc16-only"];\nconst maxBlockSizes: readonly ZmodemMaxBlockSize[] = [1024, 2048, 4096, 8192];\n\nexport default function ZmodemConfigForm({ config, onChange }: ZmodemConfigFormProps) {\n  const { t } = useTranslation();\n  return (\n    <div className={styles.form}>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configSendSettings")} · {t("transfer.configCrcPolicy")}\n        </label>\n        <div className={styles.btnRow}>\n          {crcPolicies.map((crcPolicy) => (\n            <button\n              key={crcPolicy}\n              className={`${styles.optionBtn} liquid-glass-button ${config.send.crcPolicy === crcPolicy ? "active" : ""}`}\n              onClick={() => onChange({ ...config, send: { ...config.send, crcPolicy } })}\n            >\n              {t(`transfer.configMode.${crcPolicy}`)}\n            </button>\n          ))}\n        </div>\n      </div>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configSendSettings")} · {t("transfer.configMaxBlockSize")}\n        </label>\n        <div className={styles.btnRow}>\n          {maxBlockSizes.map((maxBlockSize) => (\n            <button\n              key={maxBlockSize}\n              className={`${styles.optionBtn} liquid-glass-button ${config.send.maxBlockSize === maxBlockSize ? "active" : ""}`}\n              onClick={() => onChange({ ...config, send: { ...config.send, maxBlockSize } })}\n            >\n              {maxBlockSize / 1024}K\n            </button>\n          ))}\n        </div>\n      </div>\n      <div className={styles.group}>\n        <label className={styles.groupLabel}>\n          {t("transfer.configReceiveSettings")} · {t("transfer.configCrcCapability")}\n        </label>\n        <div className={styles.btnRow}>\n          {crcCapabilities.map((crcCapability) => (\n            <button\n              key={crcCapability}\n              className={`${styles.optionBtn} liquid-glass-button ${config.receive.crcCapability === crcCapability ? "active" : ""}`}\n              onClick={() => onChange({ ...config, receive: { crcCapability } })}\n            >\n              {t(`transfer.configMode.${crcCapability}`)}\n            </button>\n          ))}\n        </div>\n      </div>\n    </div>\n  );\n}\n''',
)

write(
    "src/components/FileTransfer/protocol-config/ProtocolConfigForm.tsx",
    '''import type { TransferConfig } from "../../../types/transfer";\nimport XmodemConfigForm from "./forms/XmodemConfigForm";\nimport YmodemConfigForm from "./forms/YmodemConfigForm";\nimport ZmodemConfigForm from "./forms/ZmodemConfigForm";\n\ninterface ProtocolConfigFormProps {\n  config: TransferConfig;\n  onChange: (config: TransferConfig) => void;\n}\n\n/** 只渲染本机角色真正能决定、且后端状态机会消费的协议参数。 */\nexport default function ProtocolConfigForm({ config, onChange }: ProtocolConfigFormProps) {\n  switch (config.protocol) {\n    case "ymodem":\n      return <YmodemConfigForm config={config} onChange={onChange} />;\n    case "xmodem":\n      return <XmodemConfigForm config={config} onChange={onChange} />;\n    case "zmodem":\n      return <ZmodemConfigForm config={config} onChange={onChange} />;\n    case "sftp":\n      return null;\n  }\n}\n''',
)

# 4. TransferContext converts UI state to the exact role-specific wire contract.
cpath = "src/context/TransferContext.tsx"
c = read(cpath)
c = c.replace(
    "  ProtocolType,\n  TransferConfig,",
    "  ProtocolType,\n  ReceiveProtocolOptions,\n  SendProtocolOptions,\n  TransferConfig,",
    1,
)
helper_marker = "interface TransferContextValue {"
helpers = '''function sendProtocolOptions(config: TransferConfig): SendProtocolOptions {\n  switch (config.protocol) {\n    case "ymodem":\n      return { protocol: "ymodem", blockSize: config.send.blockSize };\n    case "xmodem":\n      return { protocol: "xmodem", blockSize: config.send.blockSize };\n    case "zmodem":\n      return {\n        protocol: "zmodem",\n        crcPolicy: config.send.crcPolicy,\n        maxBlockSize: config.send.maxBlockSize,\n      };\n    case "sftp":\n      return { protocol: "sftp" };\n  }\n}\n\nfunction receiveProtocolOptions(config: TransferConfig): ReceiveProtocolOptions {\n  switch (config.protocol) {\n    case "ymodem":\n      return { protocol: "ymodem" };\n    case "xmodem":\n      return { protocol: "xmodem", checkMode: config.receive.checkMode };\n    case "zmodem":\n      return { protocol: "zmodem", crcCapability: config.receive.crcCapability };\n    case "sftp":\n      return { protocol: "sftp" };\n  }\n}\n\n'''
if helpers not in c:
    c = c.replace(helper_marker, helpers + helper_marker, 1)
old_start = '''        let ack: TransferStartAck;\n        if (direction === "send") {\n          const request: FileTransferSendRequest = {\n            sessionId,\n            protocol: config.protocol,\n            filePaths: filePaths ?? [],\n          };\n          if (config.protocol === "ymodem") {\n            request.blockSize = config.blockSize;\n          }\n          ack = await startFileTransfer("send", request);\n        } else {\n          const request: FileTransferReceiveRequest = {\n            sessionId,\n            protocol: config.protocol,\n            downloadDir: downloadDir ?? "",\n            remotePaths: [],\n          };\n          if (config.protocol === "ymodem") {\n            request.blockSize = config.blockSize;\n          }\n          ack = await startFileTransfer("receive", request);\n        }'''
new_start = '''        let ack: TransferStartAck;\n        if (direction === "send") {\n          const request: FileTransferSendRequest = {\n            sessionId,\n            protocolOptions: sendProtocolOptions(config),\n            filePaths: filePaths ?? [],\n          };\n          if (config.protocol === "sftp") request.remoteDir = config.remotePath;\n          ack = await startFileTransfer("send", request);\n        } else {\n          const request: FileTransferReceiveRequest = {\n            sessionId,\n            protocolOptions: receiveProtocolOptions(config),\n            downloadDir: downloadDir ?? "",\n            remotePaths: [],\n          };\n          ack = await startFileTransfer("receive", request);\n        }'''
if old_start not in c:
    raise RuntimeError("TransferContext legacy request block not found")
c = c.replace(old_start, new_start, 1)
write(cpath, c)

# 5. FileManager SFTP requests use the same tagged contract.
fpath = "src/components/FileManager/hooks/useFileManager.ts"
f = read(fpath)
count = f.count("protocol: 'sftp',")
if count < 4:
    raise RuntimeError(f"expected at least four SFTP request literals, found {count}")
f = f.replace("protocol: 'sftp',", "protocolOptions: { protocol: 'sftp' },")
write(fpath, f)

# 6. Add paired translations without reordering existing keys.
translations = {
    "src/i18n/locales/en-US.json": {
        "configSendSettings": "Send settings",
        "configReceiveSettings": "Receive settings",
        "configChecksumRequest": "Checksum request",
        "configCrcPolicy": "CRC policy",
        "configMaxBlockSize": "Maximum block size",
        "configCrcCapability": "CRC capability",
        "configMode": {
            "auto": "Auto",
            "crc16": "CRC16",
            "checksum": "Checksum",
            "crc32-required": "Require CRC32",
            "crc16-only": "CRC16 only",
        },
    },
    "src/i18n/locales/zh-CN.json": {
        "configSendSettings": "发送设置",
        "configReceiveSettings": "接收设置",
        "configChecksumRequest": "校验请求",
        "configCrcPolicy": "CRC 策略",
        "configMaxBlockSize": "最大块大小",
        "configCrcCapability": "CRC 能力",
        "configMode": {
            "auto": "自动",
            "crc16": "CRC16",
            "checksum": "校验和",
            "crc32-required": "要求 CRC32",
            "crc16-only": "仅 CRC16",
        },
    },
}
for path, additions in translations.items():
    data = json.loads(read(path))
    transfer = data["transfer"]
    for key, value in additions.items():
        transfer[key] = value
    write(path, json.dumps(data, ensure_ascii=False, indent=2) + "\n")

# 7. Upgrade the lifecycle contract to assert the role-aware model instead of the removed factory.
lpath = "scripts/test-file-transfer-lifecycle.mjs"
l = read(lpath)
l = l.replace(
    'assert.match(protocol, /Option<Box<dyn SerialTransferProtocol>>/);',
    'assert.doesNotMatch(protocol, /create_protocol\\(/, "role-aware construction must not regress to a generic protocol factory");',
    1,
)
frontend_anchor = 'assert.doesNotMatch(frontendTypes, /is_file_start|is_file_complete|is_batch_complete|__batch_complete__/);'
frontend_checks = '''\nassert.match(frontendTypes, /export type SendProtocolOptions[\\s\\S]*protocol: "ymodem"[\\s\\S]*protocol: "xmodem"[\\s\\S]*protocol: "zmodem"[\\s\\S]*protocol: "sftp"/);\nassert.match(frontendTypes, /export type ReceiveProtocolOptions[\\s\\S]*checkMode: XmodemReceiveCheckMode[\\s\\S]*crcCapability: ZmodemReceiveCrcCapability/);\nassert.match(frontendTypes, /interface FileTransferSendRequest[\\s\\S]*protocolOptions: SendProtocolOptions/);\nassert.match(frontendTypes, /interface FileTransferReceiveRequest[\\s\\S]*protocolOptions: ReceiveProtocolOptions/);\nassert.doesNotMatch(frontendTypes, /FileTransferSendRequest[\\s\\S]{0,260}\\n  protocol: string/);\nassert.doesNotMatch(frontendTypes, /FileTransferReceiveRequest[\\s\\S]{0,300}\\n  blockSize\\?/);'''
if frontend_checks not in l:
    l = l.replace(frontend_anchor, frontend_anchor + frontend_checks, 1)
trans_anchor = 'assert.doesNotMatch(transmission, /state\\.activeSessionId|state\\.activeProtocol/);'
trans_checks = '''\nassert.match(transmission, /<ProtocolConfigForm config=\\{config\\} onChange=\\{setConfig\\}/);\nassert.match(context, /protocolOptions: sendProtocolOptions\\(config\\)/);\nassert.match(context, /protocolOptions: receiveProtocolOptions\\(config\\)/);\nassert.doesNotMatch(context, /request\\.blockSize/);'''
if trans_checks not in l:
    l = l.replace(trans_anchor, trans_anchor + trans_checks, 1)
protocol_anchor = 'assert.match(protocol, /真正跨传输方式的扩展点是 `kernel::file_transfer::FileTransfer`/);'
backend_checks = '''\n\nconst roleConfig = await source("src-tauri/src/transfer/config.rs");\nassert.match(roleConfig, /pub enum SendProtocolOptions[\\s\\S]*Ymodem[\\s\\S]*Xmodem[\\s\\S]*Zmodem[\\s\\S]*Sftp/);\nassert.match(roleConfig, /pub enum ReceiveProtocolOptions[\\s\\S]*Ymodem[\\s\\S]*Xmodem[\\s\\S]*Zmodem[\\s\\S]*Sftp/);\nassert.match(roleConfig, /pub enum XModemReceiveMode[\\s\\S]*Auto[\\s\\S]*Crc16[\\s\\S]*Checksum/);\nassert.match(roleConfig, /pub enum ZModemCrcPolicy[\\s\\S]*Crc32Required/);\n\nconst xmodem = await source("src-tauri/src/transfer/xmodem.rs");\nassert.doesNotMatch(xmodem, /g_mode|G.*1K|1K.*G/);\nassert.match(xmodem, /Sender \\{ block_size: usize \\}/);\nassert.match(xmodem, /Receiver \\{ check_mode: XModemReceiveMode \\}/);\n\nconst zmodem = await source("src-tauri/src/transfer/zmodem.rs");\nassert.match(zmodem, /ZModemRole[\\s\\S]*Sender[\\s\\S]*crc_policy: ZModemCrcPolicy[\\s\\S]*Receiver[\\s\\S]*crc_capability: ZModemReceiveCrcCapability/);\nassert.match(zmodem, /receiver_crc32[\\s\\S]*ZModemCrcPolicy::Auto => receiver_crc32/);\nassert.match(zmodem, /ZModemCrcPolicy::Crc32Required if receiver_crc32 => true/);\nassert.match(zmodem, /rinit_flags\\[ZF0\\] = if use_crc32 \\{ CANFC32 \\} else \\{ 0 \\}/);'''
if backend_checks not in l:
    l = l.replace(protocol_anchor, protocol_anchor + backend_checks, 1)
session_anchor = 'assert.doesNotMatch(sessionStore, /pub active_transfer_id:|pub transfer_cancel:|pub cancel_transfer_tx:/);'
session_checks = '''\nassert.doesNotMatch(sessionStore, /reserve_inline_transfer[\\s\\S]{0,260}oneshot::Sender/);\nassert.match(sessionStore, /reserve_inline_transfer[\\s\\S]{0,260}Arc<std::sync::atomic::AtomicBool>/);'''
if session_checks not in l:
    l = l.replace(session_anchor, session_anchor + session_checks, 1)
write(lpath, l)

# 8. Architecture documentation: explicitly record ownership and negotiation boundaries.
dpath = "docs/modules/TRANSFER.md"
d = read(dpath)
heading = "## Role-aware modem configuration"
if heading not in d:
    d += '''\n\n## Role-aware modem configuration\n\nThe transfer command boundary uses a tagged `protocolOptions` object rather than flattened modem fields. Sending and receiving are separate Rust/TypeScript unions so a setting owned by one local role cannot silently affect the opposite role. There is no compatibility parser for the removed `blockSize`/checksum/streaming top-level request shape.\n\n- **YMODEM**: the sender may choose 128/1024-byte data blocks; the receiver follows the protocol handshake and does not reuse that sender preference.\n- **XMODEM**: sender block size (SOH=128, STX=1024) is independent of checksum negotiation. The receiver controls whether it requests CRC16 with `C`, checksum with `NAK`, or starts in automatic CRC16-with-fallback mode. `G` is not an XMODEM-1K selector.\n- **ZMODEM**: the receiver advertises CRC32 capability with `ZRINIT.CANFC32`. Sender `auto` uses CRC32 only when advertised, `crc16` stays on CRC16, and `crc32-required` fails when the peer cannot provide CRC32. Maximum sender data block size is a sender-side limit.\n\nInline transfer cancellation has one owner: `TransferScheduler` creates and stores the same `Arc<AtomicBool>` token consumed by the running transfer. The obsolete oneshot cancellation compatibility parameter is removed; oneshot channels that remain in the orchestrator are lifecycle start gates, not cancellation state.\n'''
write(dpath, d)

print("final role-aware transfer migration applied")
