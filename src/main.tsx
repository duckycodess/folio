import React from "react";
import ReactDOM from "react-dom/client";
// DM Sans: interface, body and labels. Manrope: headings.
import "@fontsource/dm-sans/latin-400.css";
import "@fontsource/dm-sans/latin-500.css";
import "@fontsource/dm-sans/latin-600.css";
import "@fontsource/dm-sans/latin-700.css";
import "@fontsource/manrope/latin-600.css";
import "@fontsource/manrope/latin-700.css";
import "@fontsource/manrope/latin-800.css";
// Other Latin-script characters in file names; loaded only when a page uses them.
import "@fontsource/dm-sans/latin-ext-400.css";
import "@fontsource/dm-sans/latin-ext-500.css";
import "@fontsource/dm-sans/latin-ext-600.css";
import "@fontsource/dm-sans/latin-ext-700.css";
import "@fontsource/manrope/latin-ext-600.css";
import "@fontsource/manrope/latin-ext-700.css";
import "@fontsource/manrope/latin-ext-800.css";
import App from "./App";
import { applyTheme, loadTheme } from "./app/theme";
import "./styles/index.css";

// Before the first render, so a saved theme never flashes the other one.
applyTheme(loadTheme(), document.documentElement);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
