import { createApp } from "vue";
import "dockview-vue/dist/styles/dockview.css";
import App from "./App.vue";
import LogPanel from "./panels/LogPanel.vue";
import WatchPanel from "./panels/WatchPanel.vue";
import SpansPanel from "./panels/SpansPanel.vue";
import CommandsPanel from "./panels/CommandsPanel.vue";

const app = createApp(App);
// dockview-vue resuelve los paneles por nombre registrado
app.component("log-panel", LogPanel);
app.component("watch-panel", WatchPanel);
app.component("spans-panel", SpansPanel);
app.component("cmds-panel", CommandsPanel);
app.mount("#app");
