<script setup>
// Panel de Spans (UI-31, primera versión): la tabla con percentiles.
// El flamegraph y la timeline en canvas llegan con F5 completo.
import { inject, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";

defineProps(["params"]);

const tick = inject("mole-tick");
const rows = ref([]);
let timer = null;

async function refresh() {
  try {
    rows.value = await invoke("span_snapshot");
  } catch {
    /* sin fuente */
  }
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 500);
});
onUnmounted(() => clearInterval(timer));
watch(() => tick.value?.seq, () => {}, { flush: "post" });

function us(v) {
  if (v == null) return "—";
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(2) + " s";
  if (v >= 1000) return (v / 1000).toFixed(2) + " ms";
  return v + " µs";
}
</script>

<template>
  <div class="spans-panel data">
    <table>
      <thead>
        <tr>
          <th>span</th>
          <th class="num">n</th>
          <th class="num">total</th>
          <th class="num">media</th>
          <th class="num">p50</th>
          <th class="num">p95</th>
          <th class="num">p99</th>
          <th class="num">max</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rows" :key="r.sym">
          <td>{{ r.name }}</td>
          <td class="num">{{ r.n }}</td>
          <td class="num">{{ us(r.total_us) }}</td>
          <td class="num">{{ us(Math.round(r.mean_us)) }}</td>
          <td class="num">{{ us(r.p50_us) }}</td>
          <td class="num">{{ us(r.p95_us) }}</td>
          <td class="num hot">{{ us(r.p99_us) }}</td>
          <td class="num hot">{{ us(r.max_us) }}</td>
        </tr>
        <tr v-if="rows.length === 0">
          <td colspan="8" class="dim">sin spans todavía</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.spans-panel {
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
.hot {
  color: var(--lvl-warn);
}
.dim {
  color: var(--text-2);
}
</style>
