/**
 * 翻译类型接口
 *
 * 覆盖 app、serial、transfer、settings、common 五个命名空间。
 * 确保中英文语言文件键结构一致。
 */

export interface AppTranslation {
  title: string;
  version: string;
}

export interface SerialTranslation {
  title: string;
  port: string;
  baudRate: string;
  dataBits: string;
  parity: string;
  stopBits: string;
  flowControl: string;
  connect: string;
  disconnect: string;
  refresh: string;
  connected: string;
  disconnected: string;
  connecting: string;
  noPorts: string;
  portOccupied: string;
  deviceDisconnected: string;
  scanning: string;
}

export interface TransferTranslation {
  title: string;
  // ── Config column ──
  config: string;
  protocol: string;
  selectProtocol: string;
  configTitle: string;
  connectionStatus: string;
  connected: string;
  disconnected: string;
  // ── Actions ──
  send: string;
  receive: string;
  sendFiles: string;
  receiveFiles: string;
  selectFiles: string;
  filesSelected: string;
  downloadDir: string;
  startTransfer: string;
  cancel: string;
  // ── Config: shared labels ──
  configBlockSize: string;
  configBlockSize128: string;
  configBlockSize1K: string;
  configChecksumMode: string;
  configChecksumStandard: string;
  configChecksumCRC16: string;
  configChecksumCRC32: string;
  configInitChar: string;
  configInitNak: string;
  configInitCRC: string;
  configWindowSize: string;
  configResumeEnabled: string;
  configCompression: string;
  configStreaming: string;
  // ── Progress column ──
  sending: string;
  receiving: string;
  complete: string;
  failed: string;
  cancelled: string;
  speed: string;
  eta: string;
  noActiveTransfer: string;
  transferProgress: string;
  fileXOfY: string;
  filesFailed: string;
  filesSkipped: string;
  filesSkippedMsg: string;
  partialSuccess: string;
  batchTitle: string;
  // ── History column ──
  history: string;
  fileName: string;
  direction: string;
  size: string;
  status: string;
  time: string;
  noHistory: string;
  clearHistory: string;
  filterAll: string;
  filterProtocol: string;
  filterDirection: string;
  filterStatus: string;
  directionLabel_send: string;
  directionLabel_receive: string;
  // ── Misc ──
  dropHere: string;
  transferringBanner: string;
  transferringStatus: string;
  // ── Protocol metadata ──
  protocols: {
    ymodem: ProtocolLabels;
    xmodem: ProtocolLabels;
    zmodem: ProtocolLabels;
  };
}

export interface ProtocolLabels {
  name: string;
  description: string;
}

export interface SettingsTranslation {
  title: string;
  language: string;
  switchToEn: string;
  switchToZh: string;
  theme: string;
}

export interface CommonTranslation {
  ok: string;
  cancel: string;
  close: string;
  error: string;
  warning: string;
  info: string;
  success: string;
  confirm: string;
  retry: string;
}

/** 完整翻译资源类型 */
export interface TranslationResources {
  app: AppTranslation;
  serial: SerialTranslation;
  transfer: TransferTranslation;
  settings: SettingsTranslation;
  common: CommonTranslation;
}
