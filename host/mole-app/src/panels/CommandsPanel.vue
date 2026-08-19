<script setup>
// Panel de Comandos (FEAT-27, REC-30): la UI se genera desde lo que el
// firmware declaró, sin configuración en el desktop.
import { inject, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n.js";

defineProps(["params"]);

const tick = inject("mole-tick");
const cmds = ref([]);
const values = ref({});
const status = ref("");
let timer = null;

async function refresh() {
  try {
    const list = await invoke("command_list");
    cmds.value = list;
    for (const c of list) {
      if (!(c.cmd_id in values.value)) {
        values.value[c.cmd_id] = c.min ?? 0;
      }
    }
  } catch {
    /* sin fuente */
  }
}

onMounted(() => {
  refresh();
  timer = setInterval(refresh, 1000);
});
onUnmounted(() => clearInterval(timer));
watch(() => tick.value?.catalogSyms, refresh);

async function run(c) {
  status.value = "";
  try {
    await invoke("send_command", {
      cmdId: c.cmd_id,
      argType: c.arg_type,
      value: c.arg_type === 0 ? null : Number(values.value[c.cmd_id]),
    });
    status.value = `→ ${c.name} ${t("sent")}`;
  } catch (e) {
    status.value = String(e);
  }
}
</script>

<template>
  <div class="cmds-panel">
    <div v-for="c in cmds" :key="c.cmd_id" class="cmd-row">
      <button class="cold run" @click="run(c)">{{ c.name }}</button>
      <template v-if="c.arg_type !== 0">
        <input
          type="range"
          v-if="c.min != null && c.max != null"
          :min="c.min"
          :max="c.max"
          :step="c.arg_type === 9 ? 0.01 : 1"
          v-model="values[c.cmd_id]"
        />
        <input class="val data" size="6" v-model="values[c.cmd_id]" />
        <span class="range data">[{{ c.min }} … {{ c.max }}]</span>
      </template>
    </div>
    <p v-if="cmds.length === 0" class="dim data">
      {{ t("noCommands") }}
    </p>
    <p v-if="status" class="status data">{{ status }}</p>
  </div>
</template>

<style scoped>
.cmds-panel {
  height: 100%;
  overflow: auto;
  background: var(--bg-1);
  padding: 8px;
}
.cmd-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.run {
  min-width: 120px;
  text-align: left;
}
.val {
  text-align: right;
}
.range,
.dim {
  color: var(--text-2);
}
input[type="range"] {
  flex: 1;
  max-width: 220px;
  accent-color: var(--lvl-debug);
}
.status {
  color: var(--text-1);
  border-top: 1px solid var(--border);
  padding-top: 6px;
}
</style>
