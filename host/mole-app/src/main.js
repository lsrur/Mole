import { createApp } from "vue";
import "dockview-vue/dist/styles/dockview.css";
import App from "./App.vue";
import LogPanel from "./panels/LogPanel.vue";
import WatchPanel from "./panels/WatchPanel.vue";

const app = createApp(App);
// dockview-vue resuelve los paneles por nombre registrado
app.component("log-panel", LogPanel);
app.component("watch-panel", WatchPanel);
app.mount("#app");
