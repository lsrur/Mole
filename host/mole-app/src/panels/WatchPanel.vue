<script setup>
// Panel de Watch (F2-B06, primera versión): tabla con stats incrementales
// del store (UI-27) y sparkline inline en canvas (FEAT-09).
import { inject, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";

defineProps(["params"]);

const tick = inject("mole-tick");
const rows = ref([]);
let timer = null;

async function refresh() {
  try {
    rows.value = await invoke("watch_snapshot");
  } catch {
    /* sin fuente todavía */
  }
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 500);
});
onUnmounted(() => clearInterval(timer));

// refrescar al toque cuando el tick anuncia watches cambiados
watch(
  () => tick.value?.watches?.length,
  (n) => {
    if (n) refresh();
  },
);

function spark(history, w = 96, h = 16) {
  if (!history || history.length < 2) return "";
  const min = Math.min(...history);
  const max = Math.max(...history);
  const span = max - min || 1;
  return history
    .map((v, i) => {
      const x = (i / (history.length - 1)) * w;
      const y = h - 2 - ((v - min) / span) * (h - 4);
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

function fmt(v) {
  if (v == null) return "—";
  if (Number.isInteger(v)) return String(v);
  return Math.abs(v) >= 1000 ? v.toFixed(1) : v.toPrecision(4);
}
</script>

<template>
  <div class="watch-panel data">
    <table>
      <thead>
        <tr>
          <th>símbolo</th>
          <th class="num">valor</th>
          <th></th>
          <th class="num">min</th>
          <th class="num">max</th>
          <th class="num">media</th>
          <th class="num">σ</th>
          <th class="num">n</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rows" :key="r.id">
          <td class="name">{{ r.name }}</td>
          <td class="num watch-value">{{ fmt(r.last) }}</td>
          <td>
            <svg width="96" height="16" class="spark">
              <path :d="spark(r.history)" />
            </svg>
          </td>
          <td class="num">{{ fmt(r.min) }}</td>
          <td class="num">{{ fmt(r.max) }}</td>
          <td class="num">{{ fmt(r.mean) }}</td>
          <td class="num">{{ fmt(r.stddev) }}</td>
          <td class="num">{{ r.n }}</td>
        </tr>
        <tr v-if="rows.length === 0">
          <td colspan="8" class="dim">sin watches todavía</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.watch-panel {
  height: 100%;
  overflow: auto;
  background: var(--bg-1);
  padding: 4px 0;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th {
  text-align: left;
  color: var(--text-2);
  font-weight: 500;
  padding: 2px 8px;
  border-bottom: 1px solid var(--border);
}
td {
  padding: 2px 8px;
  height: 22px;
  border-bottom: 1px solid color-mix(in srgb, var(--border) 40%, transparent);
}
.num {
  text-align: right;
  font-family: var(--font-data);
}
.name {
  color: var(--text-0);
}
.spark path {
  fill: none;
  stroke: var(--lvl-debug);
  stroke-width: 1;
}
.dim {
  color: var(--text-2);
}
</style>
