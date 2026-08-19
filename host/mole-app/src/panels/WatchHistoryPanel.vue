<script setup>
// Ventana emergente de historial de un watch (FEAT-09): tabla con t del
// stream, fecha/hora derivada del ancla stream→pared, y el gráfico de la
// serie. En vivo: se refresca sola mientras lleguen valores.
import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n.js";

const sym = Number(new URLSearchParams(location.search).get("sym") ?? 0);
const name = ref(`#${sym}`);
const total = ref(0);
const points = ref([]); // [[t_us, v], ...] cronológico
const anchor = ref(null);
let timer = null;

async function refresh() {
  try {
    const h = await invoke("watch_history", { sym, limit: 1000 });
    name.value = h.name;
    total.value = h.total;
    points.value = h.points;
    anchor.value = h.anchor;
  } catch {
    /* sin datos todavía */
  }
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 500);
});
onUnmounted(() => clearInterval(timer));

// tabla: lo más nuevo arriba; el DOM se limita, el gráfico usa todo
const rowsDesc = computed(() => points.value.slice(-300).reverse());

function wallStr(tUs) {
  const a = anchor.value;
  if (!a) return "—";
  const ms = a.wallMs + (tUs - a.tUs) / 1000;
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const mmm = String(Math.floor(ms) % 1000).padStart(3, "0");
  return `${d.toLocaleDateString()} ${hh}:${mm}:${ss}.${mmm}`;
}

function fmt(v) {
  if (v == null) return "—";
  if (Number.isInteger(v)) return String(v);
  return Math.abs(v) >= 1000 ? v.toFixed(1) : v.toPrecision(5);
}

// gráfico: path en coordenadas del viewBox, escalado no uniforme
const W = 600;
const H = 150;
const range = computed(() => {
  let min = Infinity;
  let max = -Infinity;
  for (const [, v] of points.value) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  return { min, max, span: max - min || 1 };
});

const path = computed(() => {
  const pts = points.value;
  if (pts.length < 2) return "";
  const { min, span } = range.value;
  const t0 = pts[0][0];
  const tspan = pts[pts.length - 1][0] - t0 || 1;
  let d = "";
  for (let i = 0; i < pts.length; i++) {
    const x = ((pts[i][0] - t0) / tspan) * W;
    const y = H - 4 - ((pts[i][1] - min) / span) * (H - 8);
    d += `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
  }
  return d;
});
</script>

<template>
  <div class="hist-panel">
    <div class="head data">
      <strong>{{ name }}</strong>
      <span class="dim">n = {{ total }}</span>
      <span class="grow"></span>
      <span class="dim">{{ points.length }} {{ t("rows") }}</span>
    </div>

    <div class="table-wrap data">
      <table>
        <thead>
          <tr>
            <th class="num">t (µs)</th>
            <th>{{ t("dateTime") }}</th>
            <th class="num">{{ t("value") }}</th>
          </tr>
        </thead>
        <tbody>
          <!-- key por posición: los t del stream pueden repetirse -->
          <tr v-for="(pt, i) in rowsDesc" :key="i">
            <td class="num">{{ pt[0] }}</td>
            <td>{{ wallStr(pt[0]) }}</td>
            <td class="num val">{{ fmt(pt[1]) }}</td>
          </tr>
          <tr v-if="points.length === 0">
            <td colspan="3" class="dim">{{ t("noWatches") }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="chart">
      <svg :viewBox="`0 0 ${W} ${H}`" preserveAspectRatio="none">
        <path :d="path" />
      </svg>
      <span class="lbl max data">{{ fmt(range.max) }}</span>
      <span class="lbl min data">{{ fmt(range.min) }}</span>
    </div>
  </div>
</template>

<style scoped>
.hist-panel {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-1);
}
.head {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 6px 10px;
  background: var(--bg-2);
  border-bottom: 1px solid var(--border);
}
.grow {
  flex: 1;
}
.table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
table {
  width: 100%;
  border-collapse: collapse;
}
th {
  position: sticky;
  top: 0;
  background: var(--bg-1);
  text-align: left;
  color: var(--text-2);
  font-weight: 500;
  padding: 2px 10px;
  border-bottom: 1px solid var(--border);
}
td {
  padding: 1px 10px;
  border-bottom: 1px solid color-mix(in srgb, var(--border) 30%, transparent);
}
.num {
  text-align: right;
  font-family: var(--font-data);
}
.val {
  color: var(--text-0);
}
.dim {
  color: var(--text-2);
}
.chart {
  position: relative;
  height: 160px;
  border-top: 1px solid var(--border);
  background: var(--bg-0);
}
.chart svg {
  width: 100%;
  height: 100%;
  display: block;
}
.chart path {
  fill: none;
  stroke: var(--lvl-debug);
  stroke-width: 1.2;
  vector-effect: non-scaling-stroke;
}
.lbl {
  position: absolute;
  right: 6px;
  color: var(--text-2);
}
.lbl.max {
  top: 4px;
}
.lbl.min {
  bottom: 4px;
}
</style>
