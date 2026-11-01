import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "@fontsource-variable/geist";
import "pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css";
import "./styles/app.css";

import { App } from "./App.tsx";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
