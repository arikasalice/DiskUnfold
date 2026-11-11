import { createRoot } from "react-dom/client";
import { Gallery } from "./Gallery";
import "../src/theme/tokens.css";
import "../src/theme/workspace.css";
import "./gallery.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing preview root");
createRoot(root).render(<Gallery />);
