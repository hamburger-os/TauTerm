export type SupportedUiLanguage = "zh-CN" | "en-US";

interface SafetyWarningTranslations {
  tftpExposureWarning: string;
}

export const safetyWarningTranslations: Record<SupportedUiLanguage, SafetyWarningTranslations> = {
  "zh-CN": {
    tftpExposureWarning: "当前配置允许网络中的设备写入并覆盖文件，请仅在可信网络中使用。",
  },
  "en-US": {
    tftpExposureWarning: "This configuration allows network clients to write and overwrite files. Use it only on a trusted network.",
  },
};