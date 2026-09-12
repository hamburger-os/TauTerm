export type SupportedUiLanguage = "zh-CN" | "en-US";

interface SafetyWarningTranslations {
  tftpExposureWarning: string;
}

export const safetyWarningTranslations: Record<SupportedUiLanguage, SafetyWarningTranslations> = {
  "zh-CN": {
    tftpExposureWarning: "此 TFTP 服务将通过非回环接口接受远程写入，并允许覆盖已有文件。请仅在可信网络中继续。",
  },
  "en-US": {
    tftpExposureWarning: "This TFTP server will accept remote writes and allow overwriting files from a non-loopback interface. Continue only on a trusted network.",
  },
};
