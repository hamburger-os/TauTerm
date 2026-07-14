import { type ReactNode } from "react";
import { SessionProvider } from "../../context/SessionContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { TransferProvider } from "../../context/TransferContext";
import { ToastProvider } from "../../context/ToastContext";

interface AppShellProps {
  children: ReactNode;
}

/**
 * 顶层布局容器 — 包裹所有 Context Provider
 */
export default function AppShell({ children }: AppShellProps) {
  return (
    <ThemeProvider>
      <SessionProvider>
        <TransferProvider>
          <ToastProvider>
            {children}
          </ToastProvider>
        </TransferProvider>
      </SessionProvider>
    </ThemeProvider>
  );
}
