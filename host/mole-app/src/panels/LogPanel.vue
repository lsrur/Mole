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

// ---- filtros siempre visibles (UI-14): se aplican al tipear ----
const lvlOn = ref([true, true, true, true, true, true]);
const selTags = ref([]); // multiselección de tags
const textQ = ref("");
const useRegex = ref(false);
let debounceTimer = null;

const tagOptions = computed(() =>
  Object.entries(symNames.value)
    .filter(([, v]) => v[1] === 2) // kind Tag (CAT-03)
    .map(([id, v]) => ({ id: Number(id), name: v[0] })),
);

const filterSpec = computed(() => {
  const spec = {};
  const mask = lvlOn.value.reduce((m, on, i) => (on ? m | (1 << i) : m), 0);
  if (mask !== 0x3f) spec.levelsMask = mask;
  if (selTags.value.length > 0) spec.tags = selTags.value;
  if (textQ.value) {
    spec.text = textQ.value;
    if (useRegex.value) spec.regex = true;
  }
  return spec;
});

// cambio de filtro: re-consultar desde el principio (o el final en follow)
watch(filterSpec, () => {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    winRows.value = [];
    winStart.value = 0;
    fetchWindow(0, 120);
  }, 120);
});

function toggleLvl(i) {
  lvlOn.value = lvlOn.value.map((v, k) => (k === i ? !v : v));
}

function toggleTag(id) {
  selTags.value = selTags.value.includes(id)
    ? selTags.value.filter((t) => t !== id)
    : [...selTags.value, id];
}

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
      filter: filterSpec.value,
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

// el tick trae logs nuevos; en follow, re-consultar la cola (UI-19).
// Con filtro activo el total visible sale de la query, no del tick.
let lastSeenIngest = 0;
watch(
  () => tick.value,
  (t) => {
    if (!t) return;
    const ingested = Number(t.logs.total);
    if (ingested !== lastSeenIngest) {
      lastSeenIngest = ingested;
      if (follow.value) {
        fetchTail();
      }
    }
  },
);

async function fetchTail() {
  const buf = await invoke("log_query", {
    filter: filterSpec.value,
    offset: 0,
    limit: 1, // solo para conocer el total filtrado
  });
  const { totalFiltered } = parseSlice(buf);
  total.value = totalFiltered;
  if (totalFiltered > 0) {
    virtualizer.value.scrollToIndex(totalFiltered - 1, { align: "end" });
    fetchWindow(Math.max(0, totalFiltered - 120), 120);
  }
}

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

// UI-13: modos de tiempo conmutables en caliente. El delta con la fila
// anterior es el que convierte el log en instrumento de medición.
const timeMode = ref("rel"); // 'abs' | 'rel' | 'dprev' | 'dmark'
const markIdx = ref(null); // fila marcada como origen (UI-18)
const showTask = ref(false);
const showCore = ref(false);

function fmtT(t, index) {
  switch (timeMode.value) {
    case "abs": {
      // µs del MCU tal cual (PR-10): correlacionar con eventos externos
      return (t / 1e6).toFixed(6);
    }
    case "dprev": {
      const prev = rowAt(index - 1);
      if (!prev) return "";
      const d = t - prev.t;
      return (d >= 0 ? "+" : "") + (d / 1000).toFixed(3) + "ms";
    }
    case "dmark": {
      if (markIdx.value === null) return "sin marca";
      const mark = rowAt(markIdx.value);
      if (!mark) return "…";
      return ((t - mark.t) / 1000).toFixed(3) + "ms";
    }
    default: {
      if (t0.value === null) return "";
      return ((t - t0.value) / 1e6).toFixed(6);
    }
  }
}

function toggleMark(index) {
  markIdx.value = markIdx.value === index ? null : index;
}

function tagName(tag) {
  if (!tag) return "";
  return symNames.value[tag]?.[0] ?? `#${tag}`;
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
    <!-- UI-14: barra de filtros siempre visible, se aplica al tipear -->
    <div class="filterbar">
      <span class="seg">
        <button
          v-for="(name, i) in LVL"
          :key="i"
          class="seg-btn"
          :class="{ on: lvlOn[i], ['lvl' + i]: lvlOn[i] }"
          @click="toggleLvl(i)"
        >
          {{ name }}
        </button>
      </span>
      <span class="seg" v-if="tagOptions.length">
        <button
          v-for="t in tagOptions"
          :key="t.id"
          class="seg-btn"
          :class="{ on: selTags.includes(t.id) || selTags.length === 0 }"
          @click="toggleTag(t.id)"
        >
          {{ t.name }}
        </button>
      </span>
      <input v-model="textQ" class="text-q" placeholder="filtrar texto…" />
      <label class="rx"><input type="checkbox" v-model="useRegex" /> .*</label>
      <select v-model="timeMode" class="tmode" title="modo de tiempo (UI-13)">
        <option value="rel">t relativo</option>
        <option value="abs">t absoluto</option>
        <option value="dprev">Δ fila anterior</option>
        <option value="dmark">Δ a la marca</option>
      </select>
      <span class="seg">
        <button class="seg-btn" :class="{ on: showTask }" @click="showTask = !showTask">task</button>
        <button class="seg-btn" :class="{ on: showCore }" @click="showCore = !showCore">core</button>
      </span>
      <span class="count">{{ total }} filas</span>
    </div>
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
              <span
                class="col-mark"
                :class="{ marked: markIdx === item.index }"
                title="marcar como origen del delta (UI-18)"
                @click="toggleMark(item.index)"
                >{{ markIdx === item.index ? "◆" : "◇" }}</span
              >
              <span class="col-t">{{ fmtT(rowAt(item.index).t, item.index) }}</span>
              <span class="col-lvl" :class="'lvl' + rowAt(item.index).level">{{
                LVL[rowAt(item.index).level] ?? "?"
              }}</span>
              <span v-if="showTask" class="col-tc">t{{ rowAt(item.index).task }}</span>
              <span v-if="showCore" class="col-tc">c{{ rowAt(item.index).core }}</span>
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
  display: flex;
  flex-direction: column;
  background: var(--bg-1);
}
.filterbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-2);
  flex-wrap: wrap;
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
  padding: 2px 7px;
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
.text-q {
  min-width: 160px;
  flex: 1;
}
.rx {
  color: var(--text-1);
  font-family: var(--font-data);
}
.count {
  color: var(--text-2);
  font-family: var(--font-data);
  font-size: var(--fs-data);
}
.scroll {
  flex: 1;
  min-height: 0;
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
.col-mark {
  color: var(--text-2);
  cursor: pointer;
  width: 14px;
}
.col-mark.marked {
  color: var(--lvl-warn);
}
.col-t {
  color: var(--text-2);
  min-width: 92px;
  text-align: right;
}
.col-tc {
  color: var(--text-2);
  min-width: 22px;
}
.tmode {
  font-family: var(--font-data);
  font-size: var(--fs-data);
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
