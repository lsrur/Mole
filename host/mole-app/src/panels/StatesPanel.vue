<script setup>
// Panel de Estados (FEAT-24, versión interina): estado vigente por máquina
// en tabla. La lane temporal en canvas llega con F5 (§7.8, UI-32).
import { inject, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n.js";

defineProps(["params"]);

const tick = inject("mole-tick");
const rows = ref([]);
let timer = null;

async function refresh() {
  try {
    rows.value = await invoke("state_snapshot");
  } catch {
    /* sin fuente */
  }
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 500);
});
onUnmounted(() => clearInterval(timer));
watch(() => tick.value?.catalogSyms, refresh);

function us(v) {
  if (v == null) return "—";
  if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + " s";
  if (v >= 1000) return (v / 1000).toFixed(1) + " ms";
  return v + " µs";
}
</script>

<template>
  <div class="states-panel data">
    <table>
      <thead>
        <tr>
          <th>{{ t("machine") }}</th>
          <th>{{ t("stateNow") }}</th>
          <th class="num">{{ t("inState") }}</th>
          <th class="num">{{ t("transitions") }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rows" :key="r.machine">
          <td>{{ r.machine }}</td>
          <td class="state-now">{{ r.state }}</td>
          <td class="num">{{ us(r.inStateUs) }}</td>
          <td class="num">{{ r.transitions }}</td>
        </tr>
        <tr v-if="rows.length === 0">
          <td colspan="4" class="dim">{{ t("noStates") }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.states-panel {
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
.state-now {
  color: var(--text-0);
  font-weight: 600;
}
.dim {
  color: var(--text-2);
}
</style>
