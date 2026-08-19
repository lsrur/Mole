<script setup>
// Diálogo de apertura de puerto (DX-03: preselección por VID conocido).
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n.js";

const emit = defineEmits(["close", "connected"]);

const ports = ref([]);
const selPort = ref("");
const baud = ref(921600);
const error = ref("");

async function refresh() {
  ports.value = await invoke("list_ports");
  const known = ports.value.find((p) => p.known);
  if (known && !selPort.value) selPort.value = known.name;
}

onMounted(refresh);

async function connect() {
  error.value = "";
  try {
    const desc = await invoke("connect_serial", {
      port: selPort.value,
      baud: Number(baud.value),
    });
    emit("connected", desc);
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="modal">
      <h3>{{ t("openPort") }}</h3>
      <label class="row">
        <span>{{ t("port") }}</span>
        <span class="hgroup">
          <select v-model="selPort" class="cold wide">
            <option v-if="ports.length === 0" disabled value="">{{ t("noPorts") }}</option>
            <option v-for="p in ports" :key="p.name" :value="p.name">
              {{ p.name }}{{ p.known ? " · " + p.known : "" }}
            </option>
          </select>
          <button class="cold" :title="t('refresh')" @click="refresh">⟳</button>
        </span>
      </label>
      <label class="row">
        <span>{{ t("baud") }}</span>
        <input v-model="baud" size="9" class="data" />
      </label>
      <p v-if="error" class="err data">{{ error }}</p>
      <div class="foot">
        <button class="cold" @click="emit('close')">{{ t("cancel") }}</button>
        <button class="cold" :disabled="!selPort" @click="connect">{{ t("connect") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.modal {
  background: var(--bg-1);
  border: 1px solid var(--border);
  border-radius: var(--radius-cold);
  min-width: 380px;
  padding: 14px 16px;
}
h3 {
  margin: 0 0 12px;
  font-size: 13px;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 10px;
}
.hgroup {
  display: flex;
  gap: 6px;
}
.wide {
  min-width: 220px;
}
.err {
  color: var(--lvl-error);
}
.foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 12px;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
</style>
