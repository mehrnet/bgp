import { render } from "solid-js/web";
import { App } from "./App.tsx";
import "./styles/tokens.css";
import "./styles/layout.css";
import "./styles/search.css";
import "./styles/results.css";
import "./styles/api-docs.css";
import "./styles/responsive.css";

const root = document.getElementById("app");
if (!root) {
  throw new Error("Missing #app mount point");
}

render(() => <App />, root);
