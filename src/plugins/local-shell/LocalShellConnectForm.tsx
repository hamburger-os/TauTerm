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

export default function LocalShellConnectForm({
  params,
  onChange,
  endpoints = [],
}: ConnectFormProps) {
  const { t } = useTranslation();
  const executable = stringParam(params, "executable");
  const cwd = stringParam(params, "cwd");
  const args = argsParam(params);
  const shellMode = stringParam(params, "shell_mode") || "auto";
  const shellKind = stringParam(params, "shell_kind") || "native";
  const presetId = stringParam(params, "preset_id");
  const customSelected = shellMode === "custom";
  const selectedShell = customSelected ? CUSTOM : (shellMode === "auto" ? "" : presetId);
  const isWsl = shellKind === "wsl";

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
              update({ ...preset.params, args, cwd: cwdForShellKind(nextKind) });
            }
          }}
        >
          <option value="">{t("localShell.auto")}</option>
          {endpoints.map(endpoint => (
            <option key={endpoint.name} value={endpoint.name}>
              {endpoint.name === "wsl-default" ? t("localShell.wslDefault") : endpoint.description}
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
