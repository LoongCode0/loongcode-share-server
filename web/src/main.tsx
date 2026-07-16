import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./theme.css";
import "highlight.js/styles/github-dark.css";

createRoot(document.getElementById("root")!).render(<App />);
