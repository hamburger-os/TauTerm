import { readFileSync } from "node:fs";

const files = [
  "src/i18n/locales/zh-CN.json",
  "src/i18n/locales/en-US.json",
  "src/context/ThemeContext.tsx",
  "src/components/Settings/panels/AppearanceSettings.tsx",
  "src/components/Settings/panels/AboutSettings.tsx",
  "src/components/Layout/SpectrumAmbientBackground.tsx",
  "src/components/Terminal/Terminal.tsx",
  "src/styles/tokens.css",
  "src/styles/global.css",
  "src/App.tsx",
  "src/App.css",
  ".agents/skills/tauterm-theme/SKILL.md",
  ".agents/skills/tauterm-theme-review/SKILL.md",
  "docs/README.md",
];

const forbidden = [
  "Google",
  "Gemini",
  "Apple",
  "Microsoft",
  "GitHub",
  "Chrome",
  "Firefox",
  "Mozilla",
  "PowerShell",
  "Git Bash",
  "Ubuntu",
  "Debian",
  "Npcap",
  "libpcap",
  "Keychain",
  "Credential Manager",
  "Secret Service",
  "Tauri",
  "React",
  "xterm",
];

const failures = [];

for (const file of files) {
  const source = readFileSync(file, "utf8");
  const lines = source.split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    for (const word of forbidden) {
      const pattern = new RegExp(`\\b${word.replace(/[.*+?^\${}()|[\\]\\]/g, "\\$&")}\\b`, "i");
      if (pattern.test(line)) {
        failures.push(`${file}:${index + 1}: forbidden UI/theme brand term "${word}"`);
      }
    }
  }
}

if (failures.length > 0) {
  console.error("Brand-neutral UI/theme copy check failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log("Brand-neutral UI/theme copy check passed.");
