import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { App } from "./App";
import { applyThemePreference, readThemePreference } from "./theme";
import "./styles/tokens.css";
import "./styles/app.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // The app is offline and local; retries only add noise to a failing IPC call.
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

// Apply the saved theme before first paint so the choice survives reloads.
applyThemePreference(readThemePreference());

const container = document.getElementById("root");
if (!container) {
  throw new Error("root element is missing from index.html");
}

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
