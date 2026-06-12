import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import enUS from "./locales/en-US.json";

// 从 localStorage 读取用户语言偏好
const savedLanguage = localStorage.getItem("tauterm-language") || "zh-CN";

i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: savedLanguage,
  fallbackLng: "zh-CN",
  interpolation: {
    escapeValue: false, // React 已处理 XSS
  },
});

export default i18n;
