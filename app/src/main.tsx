import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "@fontsource/league-gothic/400.css";
import "@fontsource/black-han-sans/400.css";
import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./styles/app.css";

import { App } from "./App.tsx";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
