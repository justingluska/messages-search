import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/app.css";
import App from "./App";
import { inTauri } from "./lib/api";

// Inside the desktop app the window uses an overlay title bar (tauri.conf.json:
// titleBarStyle "Overlay"), so the page reserves the traffic-light strip itself.
if (inTauri) document.documentElement.classList.add("overlay-titlebar");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
