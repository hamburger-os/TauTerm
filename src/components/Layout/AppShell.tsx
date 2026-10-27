import { type ReactNode } from "react";
import { SessionProvider } from "../../context/SessionContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { TransferProvider } from "../../context/TransferContext";
import { ToastProvider } from "../../context/ToastContext";
import { SplitLayoutProvider } from "../../context/SplitLayoutContext";

interface AppShellProps {
  children: ReactNode;
}

/**
 * 顶层布局容器 — 包裹所有 Context Provider。
 * SplitLayoutProvider 必须位于 SessionProvider 内部，以便把 selected Pane
 * 与 active session 语义同步；布局恢复与持久化由它统一协调。
 */
export default function AppShell({ children }: AppShellProps) {
  return (
    <ThemeProvider>
      <SessionProvider>
        <SplitLayoutProvider>
          <TransferProvider>
            <ToastProvider>
              {children}
            </ToastProvider>
          </TransferProvider>
        </SplitLayoutProvider>
      </SessionProvider>
    </ThemeProvider>
  );
}
