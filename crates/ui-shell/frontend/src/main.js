import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
// Third-party screens and layouts, registered before any layout loads.
import "./screens/index.js";

const app = mount(App, { target: document.getElementById("app") });

export default app;
