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
  sendFiles: string;
  receiveFiles: string;
  selectFiles: string;
  downloadDir: string;
  cancel: string;
  sending: string;
  receiving: string;
  complete: string;
  failed: string;
  cancelled: string;
  history: string;
  fileName: string;
  direction: string;
  size: string;
  status: string;
  time: string;
  send: string;
  receive: string;
  noHistory: string;
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
