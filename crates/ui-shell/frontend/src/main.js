import { mount, unmount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./app.css";
import App from "./App.svelte";
import VenueChooser from "./lib/VenueChooser.svelte";
import { launch, loadLaunch, untilRunning } from "./lib/launch.svelte.js";
// Third-party screens and layouts, registered before any layout loads.
import "./screens/index.js";

const target = document.getElementById("app");
const showWorkspace = () => mount(App, { target });

// The workspace needs a running venue. The main window asks for one when the
// build offers several and none is chosen; any other window waits for it.
async function begin() {
  await loadLaunch();
  if (launch.stage === "running") return showWorkspace();
  if (launch.stage === "choosing" && getCurrentWindow().label === "main") {
    const chooser = mount(VenueChooser, {
      target,
      props: {
        onChosen: () => {
          unmount(chooser);
          showWorkspace();
        },
      },
    });
    return chooser;
  }
  await untilRunning();
  return showWorkspace();
}

// A shell that cannot say where it stands still gets the workspace, which
// reports what fails.
begin().catch(showWorkspace);
