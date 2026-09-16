import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import ErrorBoundary from "./components/common/ErrorBoundary";
import { installBuiltinPlugins } from "./plugins/catalog";
import { installFrontendRuntimeDiagnostics } from "./utils/runtimeDiagnostics";
import "./styles/global.css";
import "./styles/selection-controls.css";

installBuiltinPlugins();
installFrontendRuntimeDiagnostics();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
