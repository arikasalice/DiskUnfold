import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./theme/tokens.css";
import "./theme/workspace.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("Missing app root");
}
createRoot(root).render(<App />);
