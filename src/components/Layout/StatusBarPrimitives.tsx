import type {
  ButtonHTMLAttributes,
  HTMLAttributes,
  ReactNode,
} from "react";
import styles from "./StatusBar.module.css";

export type StatusBarTone = "neutral" | "muted" | "accent" | "success" | "warning" | "danger";

function toneClass(tone: StatusBarTone): string {
  switch (tone) {
    case "muted": return styles.toneMuted;
    case "accent": return styles.toneAccent;
    case "success": return styles.toneSuccess;
    case "warning": return styles.toneWarning;
    case "danger": return styles.toneDanger;
    default: return styles.toneNeutral;
  }
}

function indicatorClass(tone: StatusBarTone): string {
  switch (tone) {
    case "neutral": return styles.indicatorNeutral;
    case "accent": return styles.indicatorAccent;
    case "success": return styles.indicatorSuccess;
    case "warning": return styles.indicatorWarning;
    case "danger": return styles.indicatorDanger;
    default: return styles.indicatorMuted;
  }
}

interface StatusBarGroupProps extends HTMLAttributes<HTMLSpanElement> {
  children: ReactNode;
}

export function StatusBarGroup({ children, className = "", ...props }: StatusBarGroupProps) {
  return (
    <span className={`${styles.primitiveGroup} ${className}`.trim()} {...props}>
      {children}
    </span>
  );
}

interface StatusBarTextProps extends HTMLAttributes<HTMLSpanElement> {
  children: ReactNode;
  tone?: StatusBarTone;
  mono?: boolean;
}

export function StatusBarText({
  children,
  tone = "neutral",
  mono = true,
  className = "",
  ...props
}: StatusBarTextProps) {
  return (
    <span
      className={`${styles.primitiveText} ${toneClass(tone)} ${mono ? styles.mono : ""} ${className}`.trim()}
      {...props}
    >
      {children}
    </span>
  );
}

interface StatusBarBadgeProps extends HTMLAttributes<HTMLSpanElement> {
  children: ReactNode;
  tone?: StatusBarTone;
}

export function StatusBarBadge({
  children,
  tone = "accent",
  className = "",
  ...props
}: StatusBarBadgeProps) {
  return (
    <span className={`${styles.primitiveBadge} ${toneClass(tone)} ${className}`.trim()} {...props}>
      {children}
    </span>
  );
}

interface StatusBarIndicatorProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: StatusBarTone;
  pulse?: boolean;
}

export function StatusBarIndicator({
  tone = "muted",
  pulse = false,
  className = "",
  ...props
}: StatusBarIndicatorProps) {
  return (
    <span
      className={`${styles.primitiveIndicator} ${indicatorClass(tone)} ${pulse ? styles.indicatorPulse : ""} ${className}`.trim()}
      aria-hidden="true"
      {...props}
    />
  );
}

interface StatusBarActionProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  tone?: StatusBarTone;
}

export function StatusBarAction({
  children,
  tone = "accent",
  className = "",
  type = "button",
  ...props
}: StatusBarActionProps) {
  return (
    <button
      type={type}
      className={`${styles.primitiveAction} ${toneClass(tone)} ${className}`.trim()}
      {...props}
    >
      {children}
    </button>
  );
}
