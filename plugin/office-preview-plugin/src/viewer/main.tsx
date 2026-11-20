import React from "react";
import ReactDOM from "react-dom/client";
import { configureDefaultFonts } from "@betteroffice/docx/layout";
import * as bundledFonts from "@betteroffice/fonts";
import "@betteroffice/docx-react/styles.css";
import ViewerApp from "./ViewerApp";

// Use local same-origin bundled fonts exclusively (zero external Google Fonts / CDN traffic).
configureDefaultFonts({ fonts: bundledFonts });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ViewerApp />
  </React.StrictMode>,
);
