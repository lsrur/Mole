<script setup>
// Shell de mole-app (F2-B01). El estado vive en Rust (ARQ-02): acá solo
// llegan el tick de 30 Hz y las ventanas binarias que se piden.
import { computed, onMounted, onUnmounted, ref, shallowRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const tick = shallowRef(null); // el último Tick entero; shallow a propósito
const ports = ref([]);
const selPort = ref("");
const baud = ref(921600);
const replayPath = ref("");
const source = ref("sin fuente");
const error = ref("");

let unlisten = null;

onMounted(async () => {
  unlisten = await listen("mole:tick", (e) => {
    tick.value = e.payload;
  });
  await refreshPorts();
});

onUnmounted(() => unlisten && unlisten());

async function refreshPorts() {
  ports.value = await invoke("list_ports");
  const known = ports.value.find((p) => p.known);
  if (known && !selPort.value) selPort.value = known.name; // DX-03
}

async function connect() {
  error.value = "";
  try {
    source.value = await invoke("connect_serial", {
      port: selPort.value,
      baud: Number(baud.value),
    });
  } catch (e) {
    error.value = String(e);
  }
}

async function openReplay() {
  error.value = "";
  try {
    source.value = await invoke("open_replay", { path: replayPath.value });
  } catch (e) {
    error.value = String(e);
  }
}

function fmtRate(n) {
  if (n == null) return "—";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

const linkClass = computed(() => {
  const t = tick.value;
  if (!t) return "link-bad";
  if (t.link.gaps > 0 || t.link.drops > 0) return "link-warn";
  return t.link.recPerSec > 0 ? "link-ok" : "link-warn";
});
</script>

<template>
  <div class="shell">
    <div class="toolbar">
      <strong>Mole</strong>
      <select v-model="selPort" class="cold">
        <option v-for="p in ports" :key="p.name" :value="p.name">
          {{ p.name }}{{ p.known ? " · " + p.known : "" }}
        </option>
      </select>
      <input v-model="baud" size="8" title="baudrate (solo UART)" />
      <button class="cold" @click="connect">Conectar</button>
      <button class="cold" @click="refreshPorts">⟳</button>
      <span class="sep"></span>
      <input v-model="replayPath" placeholder="ruta a un stream crudo…" size="34" />
      <button class="cold" @click="openReplay">Replay</button>
      <span class="src">{{ source }}</span>
      <span v-if="error" class="err">{{ error }}</span>
    </div>

    <div class="center data">
      <template v-if="tick">
        <p>
          logs: <b>{{ tick.logs.total }}</b>
          <span v-if="tick.logs.firstIndex > 0">
            (datos desde t={{ tick.logs.dataSinceUs }} µs — hubo descarte)
          </span>
        </p>
        <p>catálogo: {{ tick.catalogSyms }} símbolos · watches en el último tick: {{ tick.watches.length }}</p>
        <p class="dim">los paneles Log y Watch llegan con B-03/B-04 (dockview + scroller)</p>
      </template>
      <p v-else class="dim">esperando el primer tick…</p>
    </div>

    <div class="statusbar">
      <span :class="linkClass">●</span>
      <span>{{ fmtRate(tick?.link.recPerSec) }} rec/s</span>
      <span>{{ fmtRate(tick?.link.bytesPerSec) }} B/s</span>
      <span>drops MCU: {{ tick?.link.drops ?? "—" }}</span>
      <span>huecos: {{ tick?.link.gaps ?? "—" }}</span>
      <span>raw: {{ tick?.link.rawBytes ?? 0 }} B</span>
      <span class="grow"></span>
      <span>tick #{{ tick?.seq ?? 0 }}</span>
    </div>
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
  padding: 12px;
  overflow: auto;
}
.sep {
  width: 1px;
  height: 18px;
  background: var(--border);
  margin: 0 4px;
}
.src {
  color: var(--text-1);
  margin-left: 8px;
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
