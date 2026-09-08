import { Component, ErrorInfo, ReactNode } from "react";
import GlassPanel from "./GlassPanel";
import Icon from "./Icon";
import i18n from "../../i18n";
import { reportFrontendError } from "../../utils/runtimeDiagnostics";

interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

/**
 * 错误边界组件
 *
 * 捕获子组件树中的 JavaScript 错误，防止整个应用崩溃。
 */
export default class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    reportFrontendError("react.error-boundary", error, errorInfo.componentStack || undefined);
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: null });
  };

  render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            padding: "24px",
          }}
        >
          <GlassPanel padding="lg" variant="elevated">
            <div style={{ textAlign: "center", maxWidth: "400px" }}>
              <div style={{ marginBottom: "12px" }}>
                <Icon name="warning" size="2xl" />
              </div>
              <h3
                style={{
                  color: "var(--text-primary)",
                  marginBottom: "8px",
                  fontSize: "var(--text-md)",
                }}
              >
                {i18n.t("runtimeErrors.title")}
              </h3>
              <p
                style={{
                  color: "var(--text-secondary)",
                  fontSize: "var(--text-sm)",
                  marginBottom: "16px",
                }}
              >
                {this.state.error?.message || i18n.t("runtimeErrors.unknown")}
              </p>
              <button
                onClick={this.handleRetry}
                className="liquid-primary-button"
                style={{
                  padding: "8px 24px",
                  borderRadius: "var(--radius-md)",
                  fontSize: "var(--text-sm)",
                }}
              >
                {i18n.t("runtimeErrors.retry")}
              </button>
            </div>
          </GlassPanel>
        </div>
      );
    }

    return this.props.children;
  }
}
