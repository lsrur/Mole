<script setup>
// Shell de mole-app. El estado vive en Rust (ARQ-02): acá solo llegan el
// tick de 30 Hz y las ventanas binarias que se piden. La toolbar se adapta
// al tipo de fuente: vivo (pausa/cerrar), archivo (1×/máx/paso), demo.
import { computed, onMounted, onUnmounted, provide, ref, shallowRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { DockviewVue } from "dockview-vue";
import { t } from "./i18n.js";
import PrefsModal from "./modals/PrefsModal.vue";
import PortModal from "./modals/PortModal.vue";

const tick = shallowRef(null);
const symNames = shallowRef({});
provide("mole-tick", tick);
provide("mole-syms", symNames);

const error = ref("");
const showPrefs = ref(false);
const showPort = ref(false);

// estado de fuente: viene DENTRO del tick (kind/desc/paused/replay)
const source = computed(() => tick.value?.source ?? { kind: "none" });

// refrescar nombres de símbolos cuando el catálogo crece
watch(
  () => tick.value?.catalogSyms,
  async (n, prev) => {
    if (n && n !== prev) {
      symNames.value = await invoke("sym_names");
    }
  },
);

// ---- layout (dockview) ----
let dockApi = null;
let saveTimer = null;

function defaultLayout(api) {
  api.clear();
  api.addPanel({ id: "log", component: "log-panel", title: "Log" });
  api.addPanel({
    id: "watch",
    component: "watch-panel",
    title: "Watch",
    position: { referencePanel: "log", direction: "right" },
    initialWidth: 460,
  });
  api.addPanel({
    id: "spans",
    component: "spans-panel",
    title: "Spans",
    position: { referencePanel: "watch", direction: "below" },
  });
  api.addPanel({
    id: "states",
    component: "states-panel",
    title: t("statesTitle"),
    position: { referencePanel: "spans", direction: "within" },
  });
  api.addPanel({
    id: "cmds",
    component: "cmds-panel",
    title: t("commandsTitle"),
    position: { referencePanel: "spans", direction: "below" },
    initialHeight: 160,
  });
  api.getPanel("spans")?.api.setActive();
}

function onDockReady(event) {
  dockApi = event.api;
  const saved = localStorage.getItem("mole-layout");
  if (saved) {
    try {
      dockApi.fromJSON(JSON.parse(saved));
    } catch {
      defaultLayout(dockApi);
    }
  } else {
    defaultLayout(dockApi);
  }
  dockApi.onDidLayoutChange(() => {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      try {
        localStorage.setItem("mole-layout", JSON.stringify(dockApi.toJSON()));
      } catch {
        /* sin persistencia no se rompe nada */
      }
    }, 400);
  });
}

function resetLayout() {
  localStorage.removeItem("mole-layout");
  if (dockApi) defaultLayout(dockApi);
  showPrefs.value = false;
}

async function detachWatch() {
  error.value = "";
  try {
    await invoke("detach_panel", { panel: "watch" });
  } catch (e) {
    error.value = String(e);
  }
}

// ---- fuentes ----

async function openFile() {
  error.value = "";
  try {
    const path = await openDialog({
      multiple: false,
      title: t("openFile"),
    });
    if (!path) return;
    await invoke("open_replay", { path, mode: "realtime" });
  } catch (e) {
    error.value = String(e);
  }
}

async function startDemo() {
  error.value = "";
  try {
    await invoke("demo_start", { rate: 50000 });
  } catch (e) {
    error.value = String(e);
  }
}

async function togglePause() {
  await invoke("source_pause", { paused: !source.value.paused });
}

async function closeSource() {
  await invoke("source_close");
}

// Clear disponible siempre que haya datos, con o sin fuente abierta
const hasData = computed(
  () => (tick.value?.logs?.total ?? 0) > 0 || (tick.value?.catalogSyms ?? 0) > 0,
);

async function clearData() {
  await invoke("clear_data");
}

async function setReplayMode(mode) {
  await invoke("replay_mode", { mode });
}

async function stepReplay(n) {
  await invoke("replay_step", { n });
}

// ---- tick ----
const panelMode = new URLSearchParams(location.search).get("panel");
let unlisten = null;

onMounted(async () => {
  try {
    unlisten = await listen("mole:tick", (e) => {
      tick.value = e.payload;
    });
  } catch (e) {
    error.value = "listen(mole:tick): " + String(e);
  }
});

onUnmounted(() => unlisten && unlisten());

function fmtRate(n) {
  if (n == null) return "—";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

// sin la opción theme, dockview-core aplica themeAbyss a su elemento
// interno y esas variables pisan las heredadas del contenedor; el color
// real sale de los tokens (.dock .dockview-theme-dark en tokens.css)
const moleDockTheme = { name: "mole", className: "dockview-theme-dark" };

const linkClass = computed(() => {
  const tk = tick.value;
  if (!tk || source.value.kind === "none") return "link-bad";
  if (source.value.paused) return "link-warn";
  if (tk.link.gaps > 0 || tk.link.drops > 0) return "link-warn";
  return tk.link.recPerSec > 0 ? "link-ok" : "link-warn";
});
</script>

<template>
  <!-- ventana desprendida: solo el panel, mismo contrato IPC (HOST-14) -->
  <div v-if="panelMode" class="shell">
    <component
      :is="{ watch: 'watch-panel', spans: 'spans-panel', states: 'states-panel', cmds: 'cmds-panel' }[panelMode] ?? 'log-panel'"
      class="center"
    />
    <div class="statusbar">
      <span>{{ panelMode }} — {{ t("detached") }}</span>
      <span class="grow"></span>
      <span>tick #{{ tick?.seq ?? 0 }}</span>
    </div>
  </div>

  <div v-else class="shell">
    <div class="toolbar">
      <strong>Mole</strong>

      <!-- sin fuente: las tres formas de abrir -->
      <template v-if="source.kind === 'none'">
        <button class="cold" @click="showPort = true">{{ t("openPort") }}</button>
        <button class="cold" @click="openFile">{{ t("openFile") }}</button>
        <button class="cold" @click="startDemo">{{ t("demo") }}</button>
      </template>

      <!-- con fuente: controles según el tipo -->
      <template v-else>
        <span class="src data">{{ source.desc }}</span>
        <button class="cold" @click="togglePause">
          {{ source.paused ? t("resume") : t("pause") }}
        </button>
        <template v-if="source.kind === 'file' && source.replay">
          <span class="seg">
            <button
              class="seg-btn"
              :class="{ on: source.replay.mode === 'realtime' }"
              @click="setReplayMode('realtime')"
            >
              1×
            </button>
            <button
              class="seg-btn"
              :class="{ on: source.replay.mode === 'max' }"
              @click="setReplayMode('max')"
            >
              max
            </button>
            <button
              class="seg-btn"
              :class="{ on: source.replay.mode === 'step' }"
              @click="setReplayMode('step')"
            >
              {{ t("step") }}
            </button>
          </span>
          <template v-if="source.replay.mode === 'step'">
            <button class="cold" @click="stepReplay(1)">{{ t("stepOne") }}</button>
            <button class="cold" @click="stepReplay(10)">{{ t("stepTen") }}</button>
          </template>
          <span class="data dim">{{ source.replay.pos }}/{{ source.replay.total }}</span>
        </template>
        <button class="cold" @click="closeSource">{{ t("close") }}</button>
      </template>

      <button v-if="hasData" class="cold" @click="clearData">{{ t("clear") }}</button>

      <span class="grow"></span>
      <button class="cold" @click="detachWatch">{{ t("detachWatch") }}</button>
      <button class="cold" :title="t('prefs')" @click="showPrefs = true">⚙</button>
      <span v-if="error" class="err data">{{ error }}</span>
    </div>

    <div class="center">
      <DockviewVue class="dock" :theme="moleDockTheme" @ready="onDockReady" />
    </div>

    <div class="statusbar">
      <span :class="linkClass">●</span>
      <span>{{ fmtRate(tick?.link.recPerSec) }} rec/s</span>
      <span>{{ fmtRate(tick?.link.bytesPerSec) }} B/s</span>
      <span>{{ t("dropsMcu") }}: {{ tick?.link.drops ?? "—" }}</span>
      <span>{{ t("gaps") }}: {{ tick?.link.gaps ?? "—" }}</span>
      <!-- FEAT-25: LEDs de salud declarados por el firmware, siempre visibles -->
      <span v-for="s in tick?.statuses" :key="s.name" class="led" :class="'led-' + s.level">
        ● {{ s.name }}
      </span>
      <span class="grow"></span>
      <span v-if="source.kind === 'none'" class="dim">{{ t("noSource") }}</span>
      <span>tick #{{ tick?.seq ?? 0 }}</span>
    </div>

    <PrefsModal v-if="showPrefs" @close="showPrefs = false" @reset-layout="resetLayout" />
    <PortModal v-if="showPort" @close="showPort = false" @connected="showPort = false" />
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.center {
  flex: 1;
  min-height: 0;
}
.dock {
  height: 100%;
}
.src {
  color: var(--text-1);
  max-width: 340px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.err {
  color: var(--lvl-error);
}
.dim {
  color: var(--text-2);
}
.grow {
  flex: 1;
}
.seg {
  display: inline-flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-cold);
  overflow: hidden;
}
.seg-btn {
  border: none;
  border-radius: 0;
  padding: 2px 8px;
  background: var(--bg-1);
  color: var(--text-2);
  font-family: var(--font-data);
  font-size: var(--fs-data);
}
.seg-btn + .seg-btn {
  border-left: 1px solid var(--border);
}
.seg-btn.on {
  color: var(--text-0);
  background: var(--bg-selected);
}
.led-0 {
  color: var(--text-2);
}
.led-1 {
  color: var(--link-ok);
}
.led-2 {
  color: var(--link-warn);
}
.led-3 {
  color: var(--link-bad);
}
.link-ok {
  color: var(--link-ok);
}
.link-warn {
  color: var(--link-warn);
}
.link-bad {
  color: var(--link-bad);
}
</style>
