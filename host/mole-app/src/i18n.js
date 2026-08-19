// i18n mínimo sin dependencias: un diccionario reactivo + preferencias
// persistidas (idioma, tema, tamaño de letra). Default: inglés.
import { reactive, watchEffect } from "vue";

const MESSAGES = {
  en: {
    openPort: "Open port", openFile: "Open file", demo: "Demo",
    pause: "Pause", resume: "Resume", close: "Close",
    detachWatch: "Detach Watch", prefs: "Preferences",
    language: "Language", theme: "Theme", light: "Light", dark: "Dark",
    fontSize: "Font size", normal: "Normal", large: "Large",
    restoreLayout: "Restore original layout", done: "Done",
    noSource: "no source", port: "Port", baud: "Baud rate (UART only)",
    connect: "Connect", cancel: "Cancel", refresh: "Refresh",
    realtime: "Real time", maxSpeed: "Max speed", step: "Step",
    stepOne: "Step ×1", stepTen: "Step ×10", rewind: "Rewind",
    rows: "rows", filterText: "filter text…",
    tAbs: "absolute t", tRel: "relative t", tDPrev: "Δ previous row",
    tDMark: "Δ to mark", newBack: "new — back to live",
    symbol: "symbol", value: "value", mean: "mean",
    span: "span", total: "total",
    noWatches: "no watches yet", noSpans: "no spans yet",
    noCommands: "no commands declared by the firmware", sent: "sent",
    dropsMcu: "MCU drops", gaps: "gaps", detached: "detached window",
    commandsTitle: "Commands", noPorts: "no serial ports",
    clear: "Clear", history: "History", dateTime: "date & time",
    statesTitle: "States", machine: "machine", stateNow: "state",
    inState: "time in state", transitions: "transitions",
    noStates: "no state machines yet",
  },
  es: {
    openPort: "Abrir puerto", openFile: "Abrir archivo", demo: "Demo",
    pause: "Pausar", resume: "Reanudar", close: "Cerrar",
    detachWatch: "Desprender Watch", prefs: "Preferencias",
    language: "Idioma", theme: "Tema", light: "Claro", dark: "Oscuro",
    fontSize: "Tamaño de letra", normal: "Normal", large: "Grande",
    restoreLayout: "Restaurar layout original", done: "Listo",
    noSource: "sin fuente", port: "Puerto", baud: "Baudrate (solo UART)",
    connect: "Conectar", cancel: "Cancelar", refresh: "Actualizar",
    realtime: "Tiempo real", maxSpeed: "Velocidad máxima", step: "Paso a paso",
    stepOne: "Paso ×1", stepTen: "Paso ×10", rewind: "Rebobinar",
    rows: "filas", filterText: "filtrar texto…",
    tAbs: "t absoluto", tRel: "t relativo", tDPrev: "Δ fila anterior",
    tDMark: "Δ a la marca", newBack: "nuevas — volver al vivo",
    symbol: "símbolo", value: "valor", mean: "media",
    span: "span", total: "total",
    noWatches: "sin watches todavía", noSpans: "sin spans todavía",
    noCommands: "sin comandos declarados por el firmware", sent: "enviado",
    dropsMcu: "drops MCU", gaps: "huecos", detached: "ventana desprendida",
    commandsTitle: "Comandos", noPorts: "sin puertos serie",
    clear: "Limpiar", history: "Historial", dateTime: "fecha y hora",
    statesTitle: "Estados", machine: "máquina", stateNow: "estado",
    inState: "tiempo en el estado", transitions: "transiciones",
    noStates: "sin máquinas de estado todavía",
  },
  pt: {
    openPort: "Abrir porta", openFile: "Abrir arquivo", demo: "Demo",
    pause: "Pausar", resume: "Retomar", close: "Fechar",
    detachWatch: "Destacar Watch", prefs: "Preferências",
    language: "Idioma", theme: "Tema", light: "Claro", dark: "Escuro",
    fontSize: "Tamanho da letra", normal: "Normal", large: "Grande",
    restoreLayout: "Restaurar layout original", done: "Pronto",
    noSource: "sem fonte", port: "Porta", baud: "Baud rate (só UART)",
    connect: "Conectar", cancel: "Cancelar", refresh: "Atualizar",
    realtime: "Tempo real", maxSpeed: "Velocidade máxima", step: "Passo a passo",
    stepOne: "Passo ×1", stepTen: "Passo ×10", rewind: "Rebobinar",
    rows: "linhas", filterText: "filtrar texto…",
    tAbs: "t absoluto", tRel: "t relativo", tDPrev: "Δ linha anterior",
    tDMark: "Δ até a marca", newBack: "novas — voltar ao vivo",
    symbol: "símbolo", value: "valor", mean: "média",
    span: "span", total: "total",
    noWatches: "sem watches ainda", noSpans: "sem spans ainda",
    noCommands: "sem comandos declarados pelo firmware", sent: "enviado",
    dropsMcu: "drops MCU", gaps: "lacunas", detached: "janela destacada",
    commandsTitle: "Comandos", noPorts: "sem portas seriais",
    clear: "Limpar", history: "Histórico", dateTime: "data e hora",
    statesTitle: "Estados", machine: "máquina", stateNow: "estado",
    inState: "tempo no estado", transitions: "transições",
    noStates: "sem máquinas de estado ainda",
  },
  it: {
    openPort: "Apri porta", openFile: "Apri file", demo: "Demo",
    pause: "Pausa", resume: "Riprendi", close: "Chiudi",
    detachWatch: "Stacca Watch", prefs: "Preferenze",
    language: "Lingua", theme: "Tema", light: "Chiaro", dark: "Scuro",
    fontSize: "Dimensione testo", normal: "Normale", large: "Grande",
    restoreLayout: "Ripristina layout originale", done: "Fatto",
    noSource: "nessuna fonte", port: "Porta", baud: "Baud rate (solo UART)",
    connect: "Connetti", cancel: "Annulla", refresh: "Aggiorna",
    realtime: "Tempo reale", maxSpeed: "Velocità massima", step: "Passo passo",
    stepOne: "Passo ×1", stepTen: "Passo ×10", rewind: "Riavvolgi",
    rows: "righe", filterText: "filtra testo…",
    tAbs: "t assoluto", tRel: "t relativo", tDPrev: "Δ riga precedente",
    tDMark: "Δ al segno", newBack: "nuove — torna al vivo",
    symbol: "simbolo", value: "valore", mean: "media",
    span: "span", total: "totale",
    noWatches: "ancora nessun watch", noSpans: "ancora nessuno span",
    noCommands: "nessun comando dichiarato dal firmware", sent: "inviato",
    dropsMcu: "drops MCU", gaps: "lacune", detached: "finestra staccata",
    commandsTitle: "Comandi", noPorts: "nessuna porta seriale",
    clear: "Pulisci", history: "Cronologia", dateTime: "data e ora",
    statesTitle: "Stati", machine: "macchina", stateNow: "stato",
    inState: "tempo nello stato", transitions: "transizioni",
    noStates: "ancora nessuna macchina a stati",
  },
};

// Nombrados en su propio idioma (pedido explícito).
export const LANGUAGES = [
  { id: "en", name: "English" },
  { id: "es", name: "Español" },
  { id: "pt", name: "Português" },
  { id: "it", name: "Italiano" },
];

const saved = JSON.parse(localStorage.getItem("mole-prefs") ?? "{}");

export const prefs = reactive({
  lang: saved.lang ?? "en",
  theme: saved.theme ?? "dark",
  fontSize: saved.fontSize ?? "normal",
});

watchEffect(() => {
  localStorage.setItem("mole-prefs", JSON.stringify({ ...prefs }));
  document.documentElement.dataset.theme = prefs.theme;
  document.documentElement.dataset.fontsize = prefs.fontSize;
});

export function t(key) {
  return MESSAGES[prefs.lang]?.[key] ?? MESSAGES.en[key] ?? key;
}
