import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import type { ConnectFormProps } from "../../core/plugin-registry";
import Icon from "../../components/common/Icon";
import styles from "./LocalShellConnectForm.module.css";

const CUSTOM = "__custom__";

function stringParam(params: Record<string, unknown>, key: string): string {
  return typeof params[key] === "string" ? String(params[key]) : "";
}

function argsParam(params: Record<string, unknown>): string[] {
  return Array.isArray(params.args)
    ? params.args.filter((value): value is string => typeof value === "string")
    : [];
}

function envParam(params: Record<string, unknown>): Array<{ key: string; value: string }> {
  if (!Array.isArray(params.env)) return [];
  return params.env
    .filter((entry): entry is Record<string, unknown> => typeof entry === "object" && entry !== null)
    .map(entry => ({
      key: typeof entry.key === "string" ? entry.key : "",
      value: typeof entry.value === "string" ? entry.value : "",
    }));
}

export default function LocalShellConnectForm({
  params,
  onChange,
  endpoints = [],
}: ConnectFormProps) {
  const { t } = useTranslation();
  const executable = stringParam(params, "executable");
  const cwd = stringParam(params, "cwd");
  const args = argsParam(params);
  const env = envParam(params);
  const shellMode = stringParam(params, "shell_mode") || "auto";
  const shellKind = stringParam(params, "shell_kind") || "native";
  const presetId = stringParam(params, "preset_id");
  const customSelected = shellMode === "custom";
  const selectedShell = customSelected ? CUSTOM : (shellMode === "auto" ? "" : presetId);
  const isWsl = shellKind === "wsl";
  // Environment row diagnostics are inline and local: empty rows are ignored when
  // the session starts. Malformed names would otherwise only surface as a backend
  // error after the user commits the dialog; duplicate names are advisory only
  // because the backend applies the last value without rejecting the list.
  const envKeyInvalid = env.some(
    entry => entry.key.length > 0 && !/^[A-Za-z0-9_]+$/.test(entry.key),
  );
  const envKeyDuplicate = (() => {
    const seen = new Set<string>();
    for (const entry of env) {
      const key = entry.key.trim().toUpperCase();
      if (!key) continue;
      if (seen.has(key)) return true;
      seen.add(key);
    }
    return false;
  })();

  const cwdForShellKind = (nextKind: string): string => {
    return (nextKind === "wsl") === isWsl ? cwd : "";
  };

  const update = (patch: Record<string, unknown>) => {
    onChange({
      data_mode: "text",
      encoding: "utf-8",
      send_bar_enabled: false,
      ...params,
      ...patch,
    });
  };

  const chooseExecutable = async () => {
    const selected = await open({ multiple: false, directory: false });
    if (typeof selected === "string") {
      update({
        shell_mode: "custom",
        shell_kind: "custom",
        preset_id: "",
        preset_args: [],
        shell_label: "",
        wsl_distro: "",
        executable: selected,
      });
    }
  };

  const chooseWorkingDirectory = async () => {
    const selected = await open({ multiple: false, directory: true });
    if (typeof selected === "string") update({ cwd: selected });
  };

  const updateArgument = (index: number, value: string) => {
    update({ args: args.map((argument, current) => current === index ? value : argument) });
  };

  const removeArgument = (index: number) => {
    update({ args: args.filter((_, current) => current !== index) });
  };

  const updateEnvironmentVariable = (index: number, patch: Partial<{ key: string; value: string }>) => {
    update({ env: env.map((entry, current) => (current === index ? { ...entry, ...patch } : entry)) });
  };

  const removeEnvironmentVariable = (index: number) => {
    update({ env: env.filter((_, current) => current !== index) });
  };

  return (
    <div className={styles.form}>
      <div className={styles.field}>
        <label className={styles.label}>{t("localShell.shell")}</label>
        <select
          className={`${styles.select} liquid-glass-input liquid-glass-select`}
          value={selectedShell}
          onChange={(event) => {
            const value = event.target.value;
            if (value === CUSTOM) {
              update({
                shell_mode: "custom",
                shell_kind: "custom",
                preset_id: "",
                preset_args: [],
                shell_label: "",
                wsl_distro: "",
                executable: "",
                cwd: cwdForShellKind("custom"),
              });
              return;
            }
            if (value === "") {
              update({
                shell_mode: "auto",
                shell_kind: "native",
                preset_id: "",
                preset_args: [],
                shell_label: "",
                wsl_distro: "",
                executable: "",
                cwd: cwdForShellKind("native"),
              });
              return;
            }
            const preset = endpoints.find(endpoint => endpoint.name === value);
            if (preset?.params) {
              const nextKind = preset.params.shell_kind === "wsl" ? "wsl" : "native";
              update({
                ...preset.params,
                args,
                cwd: cwdForShellKind(nextKind),
                // The WSL subsystem owns its environment; never carry native rows into it.
                ...(nextKind === "wsl" ? { env: [] } : {}),
              });
            }
          }}
        >
          <option value="">{t("localShell.auto")}</option>
          {endpoints.map(endpoint => (
            <option key={endpoint.name} value={endpoint.name}>
              {endpoint.description}
            </option>
          ))}
          <option value={CUSTOM}>{t("localShell.custom")}</option>
        </select>
      </div>

      {customSelected && (
        <div className={styles.field}>
          <label className={styles.label}>{t("localShell.executable")}</label>
          <div className={styles.row}>
            <input
              className={`${styles.input} liquid-glass-input`}
              value={executable}
              onChange={event => update({ executable: event.target.value })}
              placeholder={t("localShell.executablePlaceholder")}
            />
            <button
              type="button"
              className={`${styles.iconButton} liquid-glass-button`}
              onClick={chooseExecutable}
              aria-label={t("localShell.chooseExecutable")}
              title={t("localShell.chooseExecutable")}
            >
              <Icon name="file" size="sm" />
            </button>
          </div>
        </div>
      )}

      <div className={styles.field}>
        <label className={styles.label}>
          {t(isWsl ? "localShell.wslWorkingDirectory" : "localShell.workingDirectory")}
        </label>
        <div className={styles.row}>
          <input
            className={`${styles.input} liquid-glass-input`}
            value={cwd}
            onChange={event => update({ cwd: event.target.value })}
            placeholder={t(isWsl ? "localShell.wslHomeDirectory" : "localShell.homeDirectory")}
          />
          {!isWsl && (
            <button
              type="button"
              className={`${styles.iconButton} liquid-glass-button`}
              onClick={chooseWorkingDirectory}
              aria-label={t("localShell.chooseDirectory")}
              title={t("localShell.chooseDirectory")}
            >
              <Icon name="folder" size="sm" />
            </button>
          )}
        </div>
      </div>

      {/* 环境变量先于启动参数：两者都是“进程启动输入”，按进程创建顺序阅读。 */}
      {!isWsl && (
        <div className={styles.field}>
          <label className={styles.label}>{t("localShell.environmentVariables")}</label>
          <div className={styles.arguments}>
            {env.map((entry, index) => (
              <div className={styles.row} key={index}>
                <input
                  className={`${styles.input} liquid-glass-input`}
                  value={entry.key}
                  onChange={event => updateEnvironmentVariable(index, { key: event.target.value })}
                  placeholder={t("localShell.envKeyPlaceholder")}
                />
                <input
                  className={`${styles.input} liquid-glass-input`}
                  value={entry.value}
                  onChange={event => updateEnvironmentVariable(index, { value: event.target.value })}
                  placeholder={t("localShell.envValuePlaceholder")}
                />
                <button
                  type="button"
                  className={`${styles.iconButton} liquid-glass-button`}
                  onClick={() => removeEnvironmentVariable(index)}
                  aria-label={t("localShell.removeEnvironmentVariable")}
                  title={t("localShell.removeEnvironmentVariable")}
                >
                  <Icon name="trash" size="sm" />
                </button>
              </div>
            ))}
            <button
              type="button"
              className={`${styles.addButton} liquid-glass-button`}
              onClick={() => update({ env: [...env, { key: "", value: "" }] })}
            >
              <Icon name="plus" size="sm" />
              {t("localShell.addEnvironmentVariable")}
            </button>
          </div>
          <p className={styles.hint}>{t("localShell.environmentVariablesHint")}</p>
          {envKeyInvalid && (
            <p className={styles.warning} role="alert">
              {t("localShell.envKeyInvalid")}
            </p>
          )}
          {envKeyDuplicate && (
            <p className={styles.warning} role="alert">
              {t("localShell.envKeyDuplicate")}
            </p>
          )}
        </div>
      )}

      <div className={styles.field}>
        <label className={styles.label}>
          {t(isWsl ? "localShell.additionalArguments" : "localShell.arguments")}
        </label>
        <div className={styles.arguments}>
          {args.map((argument, index) => (
            <div className={styles.row} key={index}>
              <input
                className={`${styles.input} liquid-glass-input`}
                value={argument}
                onChange={event => updateArgument(index, event.target.value)}
                placeholder={t("localShell.argumentPlaceholder")}
              />
              <button
                type="button"
                className={`${styles.iconButton} liquid-glass-button`}
                onClick={() => removeArgument(index)}
                aria-label={t("localShell.removeArgument")}
                title={t("localShell.removeArgument")}
              >
                <Icon name="trash" size="sm" />
              </button>
            </div>
          ))}
          <button
            type="button"
            className={`${styles.addButton} liquid-glass-button`}
            onClick={() => update({ args: [...args, ""] })}
          >
            <Icon name="plus" size="sm" />
            {t("localShell.addArgument")}
          </button>
        </div>
        <p className={styles.hint}>
          {t(isWsl ? "localShell.wslArgumentsHint" : "localShell.argumentsHint")}
        </p>
      </div>
    </div>
  );
}
