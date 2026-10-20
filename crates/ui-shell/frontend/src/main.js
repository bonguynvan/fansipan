import { mount, unmount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import VenueChooser from "./lib/VenueChooser.svelte";
import { launch, loadLaunch, untilRunning } from "./lib/launch.svelte.js";
// Third-party screens and layouts, registered before any layout loads.
import "./screens/index.js";

const target = document.getElementById("app");
const showWorkspace = () => mount(App, { target });

// The workspace needs a running venue. A build that offers several asks
// for one when none is chosen; one being opened is waited for.
async function begin() {
  await loadLaunch();
  if (launch.stage === "running") return showWorkspace();
  if (launch.stage === "choosing") {
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
