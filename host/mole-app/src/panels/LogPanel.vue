<script setup>
// Panel de Log (F2-B04, primera versión): TanStack Virtual sobre ventanas
// binarias de log_query (HOST-12). Nada acá es proporcional al volumen
// (UI-03): solo se materializa la ventana visible.
import { computed, inject, onMounted, ref, shallowRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/vue-virtual";

defineProps(["params"]); // dockview panel props

const tick = inject("mole-tick");
const symNames = inject("mole-syms");

const scrollEl = ref(null);
const total = ref(0);
const winStart = ref(0);
const winRows = shallowRef([]);
const follow = ref(true); // UI-19
const t0 = ref(null);
const LVL = ["TRC", "DBG", "INF", "WRN", "ERR", "FTL"];
let fetching = false;
let pendingRange = null;

const virtualizer = useVirtualizer(
  computed(() => ({
    count: total.value,
    getScrollElement: () => scrollEl.value,
    estimateSize: () => 22, // densidad normal (UI-23)
    overscan: 30,
  })),
);

function parseSlice(buf) {
  const dv = new DataView(buf);
  const totalFiltered = Number(dv.getBigUint64(0, true));
  const rows = [];
  let o = 8;
  const dec = new TextDecoder();
  while (o + 24 <= buf.byteLength) {
    const idx = Number(dv.getBigUint64(o, true));
    const t = Number(dv.getBigUint64(o + 8, true));
    const kind = dv.getUint8(o + 16);
    const level = dv.getUint8(o + 17);
    const tag = dv.getUint16(o + 18, true);
    const task = dv.getUint8(o + 20);
    const core = dv.getUint8(o + 21);
    const len = dv.getUint16(o + 22, true);
    o += 24;
    const text = dec.decode(new Uint8Array(buf, o, len));
    o += len;
    rows.push({ idx, t, kind, level, tag, task, core, text });
  }
  return { totalFiltered, rows };
}

async function fetchWindow(start, count) {
  if (fetching) {
    pendingRange = [start, count];
    return;
  }
  fetching = true;
  try {
    const buf = await invoke("log_query", {
      filter: {},
      offset: start,
      limit: Math.max(count, 100),
    });
    const { totalFiltered, rows } = parseSlice(buf);
    total.value = totalFiltered;
    winStart.value = start;
    winRows.value = rows;
    if (t0.value === null && rows.length > 0) t0.value = rows[0].t;
  } finally {
    fetching = false;
    if (pendingRange) {
      const [s, c] = pendingRange;
      pendingRange = null;
      fetchWindow(s, c);
    }
  }
}

// pedir la ventana que el scroller está mostrando
watch(
  () => virtualizer.value.getVirtualItems(),
  (items) => {
    if (items.length === 0) return;
    const start = Math.max(0, items[0].index - 30);
    const end = items[items.length - 1].index + 30;
    if (start < winStart.value || end > winStart.value + winRows.value.length) {
      fetchWindow(start, end - start);
    }
  },
);

// el tick trae el total nuevo; en follow, ir al final (UI-19)
watch(
  () => tick.value,
  (t) => {
    if (!t) return;
    const newTotal = Number(t.logs.total);
    if (newTotal !== total.value) {
      total.value = newTotal;
      if (follow.value && newTotal > 0) {
        virtualizer.value.scrollToIndex(newTotal - 1, { align: "end" });
        fetchWindow(Math.max(0, newTotal - 120), 120);
      }
    }
  },
);

function onWheel(e) {
  if (e.deltaY < 0) follow.value = false; // congelar al scrollear arriba
}

function resumeFollow() {
  follow.value = true;
  if (total.value > 0) {
    virtualizer.value.scrollToIndex(total.value - 1, { align: "end" });
  }
}

function rowAt(index) {
  const rel = index - winStart.value;
  return rel >= 0 && rel < winRows.value.length ? winRows.value[rel] : null;
}

function fmtT(t) {
  if (t0.value === null) return "";
  return ((t - t0.value) / 1e6).toFixed(6);
}

function tagName(tag) {
  if (!tag) return "";
  return symNames.value[tag] ?? `#${tag}`;
}

onMounted(() => fetchWindow(0, 120));

const newBehind = computed(() => {
  if (follow.value || !tick.value) return 0;
  const items = virtualizer.value.getVirtualItems();
  const lastVisible = items.length ? items[items.length - 1].index : 0;
  return Math.max(0, total.value - 1 - lastVisible);
});
</script>

<template>
  <div class="log-panel">
    <div ref="scrollEl" class="scroll" @wheel="onWheel">
      <div :style="{ height: virtualizer.getTotalSize() + 'px', position: 'relative' }">
        <div
          v-for="item in virtualizer.getVirtualItems()"
          :key="item.key"
          class="log-row"
          :class="'k' + (rowAt(item.index)?.kind ?? 0)"
          :style="{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            height: item.size + 'px',
            transform: 'translateY(' + item.start + 'px)',
          }"
        >
          <template v-if="rowAt(item.index)">
            <template v-if="rowAt(item.index).kind === 0">
              <span class="col-t">{{ fmtT(rowAt(item.index).t) }}</span>
              <span class="col-lvl" :class="'lvl' + rowAt(item.index).level">{{
                LVL[rowAt(item.index).level] ?? "?"
              }}</span>
              <span class="col-tag">{{ tagName(rowAt(item.index).tag) }}</span>
              <span class="col-msg">{{ rowAt(item.index).text }}</span>
            </template>
            <span v-else class="col-marker">{{ rowAt(item.index).text }}</span>
          </template>
          <span v-else class="col-msg dim">…</span>
        </div>
      </div>
    </div>
    <button v-if="!follow" class="pill cold" @click="resumeFollow">
      ▼ {{ newBehind }} nuevas — volver al vivo
    </button>
  </div>
</template>

<style scoped>
.log-panel {
  position: relative;
  height: 100%;
  background: var(--bg-1);
}
.scroll {
  height: 100%;
  overflow-y: auto;
  contain: strict;
}
.log-row {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 0 8px;
  white-space: nowrap;
  line-height: 22px;
}
.col-t {
  color: var(--text-2);
  min-width: 92px;
  text-align: right;
}
.col-lvl {
  min-width: 28px;
  font-weight: 600;
}
.col-tag {
  color: var(--text-1);
  min-width: 60px;
}
.col-msg {
  overflow: hidden;
  text-overflow: ellipsis;
}
.col-marker {
  color: var(--lvl-warn);
  font-style: italic;
}
.k1 {
  background: color-mix(in srgb, var(--lvl-warn) 12%, transparent);
}
.lvl0 { color: var(--lvl-trace); }
.lvl1 { color: var(--lvl-debug); }
.lvl2 { color: var(--lvl-info); }
.lvl3 { color: var(--lvl-warn); }
.lvl4 { color: var(--lvl-error); }
.lvl5 { color: var(--lvl-fatal); }
.dim { color: var(--text-2); }
.pill {
  position: absolute;
  bottom: 10px;
  right: 16px;
  border-radius: 10px;
}
</style>
