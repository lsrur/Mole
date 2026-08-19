<script setup>
// Ventana de preferencias: idioma, tema, tamaño de letra y restaurar layout.
import { LANGUAGES, prefs, t } from "../i18n.js";

const emit = defineEmits(["close", "reset-layout"]);
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="modal">
      <h3>{{ t("prefs") }}</h3>

      <label class="row">
        <span>{{ t("language") }}</span>
        <select v-model="prefs.lang" class="cold">
          <option v-for="l in LANGUAGES" :key="l.id" :value="l.id">{{ l.name }}</option>
        </select>
      </label>

      <label class="row">
        <span>{{ t("theme") }}</span>
        <span class="seg">
          <button class="seg-btn" :class="{ on: prefs.theme === 'light' }" @click="prefs.theme = 'light'">
            {{ t("light") }}
          </button>
          <button class="seg-btn" :class="{ on: prefs.theme === 'dark' }" @click="prefs.theme = 'dark'">
            {{ t("dark") }}
          </button>
        </span>
      </label>

      <label class="row">
        <span>{{ t("fontSize") }}</span>
        <span class="seg">
          <button class="seg-btn" :class="{ on: prefs.fontSize === 'normal' }" @click="prefs.fontSize = 'normal'">
            {{ t("normal") }}
          </button>
          <button class="seg-btn" :class="{ on: prefs.fontSize === 'large' }" @click="prefs.fontSize = 'large'">
            {{ t("large") }}
          </button>
        </span>
      </label>

      <div class="row">
        <button class="cold" @click="emit('reset-layout')">{{ t("restoreLayout") }}</button>
      </div>

      <div class="foot">
        <button class="cold primary" @click="emit('close')">{{ t("done") }}</button>
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
  min-width: 340px;
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
.seg {
  display: inline-flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-cold);
  overflow: hidden;
}
.seg-btn {
  border: none;
  border-radius: 0;
  padding: 3px 10px;
  background: var(--bg-1);
  color: var(--text-2);
}
.seg-btn + .seg-btn {
  border-left: 1px solid var(--border);
}
.seg-btn.on {
  color: var(--text-0);
  background: var(--bg-selected);
}
.foot {
  display: flex;
  justify-content: flex-end;
  margin-top: 12px;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
</style>
