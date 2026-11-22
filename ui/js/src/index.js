// Pegamento JS de Gryphus. Expone `window.GryphusJs` con dos fábricas:
//   createTerminal(el, onData, onResize) y createEditor(el, onSave, onDirty),
// y highlightHunk(name, kinds, texts) para colorear el diff.
// Toda la lógica de la app vive en Rust (ui/src); aquí solo se envuelven
// xterm.js y CodeMirror 6. El único consumidor es ui/src/bridge/.

import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import '@xterm/xterm/css/xterm.css';

import { basicSetup } from 'codemirror';
import { EditorState, Prec } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { oneDark } from '@codemirror/theme-one-dark';
import { rust } from '@codemirror/lang-rust';
import { javascript } from '@codemirror/lang-javascript';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { sass } from '@codemirror/lang-sass';
import { less } from '@codemirror/lang-less';
import { yaml } from '@codemirror/lang-yaml';
import { xml } from '@codemirror/lang-xml';
import { sql } from '@codemirror/lang-sql';
import { java } from '@codemirror/lang-java';
import { cpp } from '@codemirror/lang-cpp';
import { go } from '@codemirror/lang-go';
import { php } from '@codemirror/lang-php';
import { StreamLanguage, ensureSyntaxTree } from '@codemirror/language';
import { tags as t, tagHighlighter, highlightTree } from '@lezer/highlight';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { dockerFile } from '@codemirror/legacy-modes/mode/dockerfile';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { powerShell } from '@codemirror/legacy-modes/mode/powershell';
import { diff } from '@codemirror/legacy-modes/mode/diff';
import { nginx } from '@codemirror/legacy-modes/mode/nginx';
import { ruby } from '@codemirror/legacy-modes/mode/ruby';
import { lua } from '@codemirror/legacy-modes/mode/lua';
import { swift } from '@codemirror/legacy-modes/mode/swift';
import { perl } from '@codemirror/legacy-modes/mode/perl';
import { r } from '@codemirror/legacy-modes/mode/r';
import { haskell } from '@codemirror/legacy-modes/mode/haskell';
import { protobuf } from '@codemirror/legacy-modes/mode/protobuf';
import { cmake } from '@codemirror/legacy-modes/mode/cmake';
import { groovy } from '@codemirror/legacy-modes/mode/groovy';
import { csharp, kotlin, scala, dart } from '@codemirror/legacy-modes/mode/clike';

import { decode } from './stream.js';
import { languageId } from './languages.js';

// ── Terminal ────────────────────────────────────────────────────────────────

let lastTerminal = null;

/** Gancho de pruebas e2e: texto del buffer de la terminal (WebGL no deja texto en el DOM). */
export function debugTerminalText() {
  if (!lastTerminal) return '';
  const buffer = lastTerminal.buffer.active;
  const lines = [];
  for (let i = 0; i < buffer.length; i++) {
    lines.push(buffer.getLine(i)?.translateToString(true) ?? '');
  }
  return lines.join('\n');
}

/**
 * Gancho de pruebas de rendimiento: ms desde enviar `token\r` al agente hasta que
 * xterm termina de procesar `echo: token` (IPC → PTY → agente → Channel → xterm).
 */
export function debugEchoLatency(agent, token) {
  return new Promise((resolve, reject) => {
    if (!lastTerminal) {
      reject(new Error('sin terminal'));
      return;
    }
    const start = performance.now();
    const sub = lastTerminal.onWriteParsed(() => {
      if (debugTerminalText().includes(`echo: ${token}`)) {
        sub.dispose();
        resolve(performance.now() - start);
      }
    });
    window.__TAURI__.core.invoke('agent_input', { req: { agent, data: `${token}\r` } });
  });
}

const DEFAULT_MONO = '"Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace';

/** Tipografía de la terminal desde las variables CSS `--mono` y `--mono-size` (configuración). */
function monoFont() {
  const style = getComputedStyle(document.documentElement);
  const family = style.getPropertyValue('--mono').trim() || DEFAULT_MONO;
  const size = parseInt(style.getPropertyValue('--mono-size'), 10) || 13;
  return { fontFamily: family, fontSize: size };
}

/**
 * `onPasteNonText()` se llama al pegar (Ctrl+V, Ctrl+Shift+V, Mayús+Insert, menú)
 * cuando el portapapeles no tiene texto: archivos copiados o una imagen. Rust
 * decide qué escribir (ui/src/store.rs → paste_non_text_into_agent).
 */
export function createTerminal(el, onData, onResize, onPasteNonText) {
  const term = new Terminal({
    ...monoFont(),
    cursorBlink: true,
    scrollback: 10000,
    theme: { background: '#15171c', foreground: '#d7dae0' },
  });
  lastTerminal = term;
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(el);
  try {
    const gl = new WebglAddon();
    gl.onContextLoss(() => gl.dispose());
    term.loadAddon(gl);
  } catch {
    // Sin WebGL: xterm usa su renderizador DOM.
  }
  term.onData((data) => onData(data));
  term.onResize(({ cols, rows }) => onResize(cols, rows));

  // xterm convierte Ctrl+V en ^V y cancela el evento: el navegador nunca pegaba.
  // Devolver `false` deja la tecla al navegador, que dispara `paste` (o `copy`).
  term.attachCustomKeyEventHandler((ev) => {
    if (ev.type !== 'keydown') return true;
    const ctrl = ev.ctrlKey && !ev.altKey && !ev.metaKey;
    const key = ev.key.toLowerCase();
    if (ctrl && key === 'v') return false;
    if (ev.shiftKey && !ev.ctrlKey && ev.key === 'Insert') return false;
    // Ctrl+C con texto seleccionado copia; sin selección sigue siendo ^C.
    if (ctrl && key === 'c' && term.hasSelection()) return false;
    return true;
  });
  // Antes que xterm (fase de captura): si no hay texto, que decida Rust.
  const onPaste = (ev) => {
    const text = ev.clipboardData?.getData('text/plain');
    if (text) return; // xterm lo pega (respeta el bracketed paste)
    ev.preventDefault();
    ev.stopImmediatePropagation();
    onPasteNonText();
  };
  el.addEventListener('paste', onPaste, true);

  const refit = () => {
    if (el.offsetWidth > 0 && el.offsetHeight > 0) {
      try {
        fit.fit();
      } catch {
        // contenedor oculto
      }
    }
  };
  const observer = new ResizeObserver(refit);
  observer.observe(el);

  // La configuración cambió la tipografía (ui/src/bridge/prefs.rs).
  const onFonts = () => {
    const { fontFamily, fontSize } = monoFont();
    term.options.fontFamily = fontFamily;
    term.options.fontSize = fontSize;
    refit();
  };
  window.addEventListener('gryphus:fonts', onFonts);

  const handle = (message) => {
    const msg = decode(message);
    if (!msg) return;
    switch (msg.kind) {
      case 'scrollback':
        term.reset();
        term.write(msg.bytes);
        break;
      case 'data':
        term.write(msg.bytes);
        break;
      case 'exited':
        term.write(
          `\r\n\x1b[90m[proceso terminado${msg.code === null ? '' : ` con código ${msg.code}`}]\x1b[0m\r\n`,
        );
        break;
      case 'failed':
        term.write(`\r\n\x1b[31m[error: ${msg.reason}]\x1b[0m\r\n`);
        break;
    }
  };

  // Solo el último canal escribe: al cambiar de agente, la salida del anterior que
  // aún estaba en vuelo se mezclaba con la del nuevo.
  let current = null;

  return {
    /** Canal de Tauri cuya salida se escribe directamente en la terminal (sin pasar por WASM). */
    channel() {
      const ch = new window.__TAURI__.core.Channel();
      current = ch;
      ch.onmessage = (message) => {
        if (current === ch) handle(message);
      };
      return ch;
    },
    reset() {
      current = null;
      term.reset();
    },
    /** Ajusta al contenedor e informa el tamaño actual. */
    fit() {
      refit();
      // También repinta: ConPTY redibuja la pantalla completa en cada resize, aunque
      // el tamaño no cambie. Al cambiar de agente eso recupera lo que el scrollback
      // ya no guarda (e2e/tests/terminal.rs → switching_agents_restores_the_screen).
      onResize(term.cols, term.rows);
    },
    focus() {
      term.focus();
    },
    write(text) {
      term.write(text);
    },
    /** Como si el usuario lo pegara: va al agente (con bracketed paste si lo pidió). */
    paste(text) {
      term.paste(text);
    },
    dispose() {
      el.removeEventListener('paste', onPaste, true);
      window.removeEventListener('gryphus:fonts', onFonts);
      observer.disconnect();
      term.dispose();
    },
  };
}

// ── Editor ──────────────────────────────────────────────────────────────────

const legacy = (mode) => () => StreamLanguage.define(mode);

/** Lenguaje → fábrica de la extensión de CodeMirror. */
const LANGUAGES = {
  rust: () => rust(),
  javascript: () => javascript(),
  jsx: () => javascript({ jsx: true }),
  typescript: () => javascript({ typescript: true }),
  tsx: () => javascript({ typescript: true, jsx: true }),
  json: () => json(),
  markdown: () => markdown(),
  python: () => python(),
  html: () => html(),
  css: () => css(),
  sass: () => sass({ indented: true }),
  scss: () => sass(),
  less: () => less(),
  yaml: () => yaml(),
  xml: () => xml(),
  sql: () => sql(),
  java: () => java(),
  cpp: () => cpp(),
  go: () => go(),
  php: () => php(),
  shell: legacy(shell),
  dockerfile: legacy(dockerFile),
  toml: legacy(toml),
  properties: legacy(properties),
  powershell: legacy(powerShell),
  diff: legacy(diff),
  nginx: legacy(nginx),
  ruby: legacy(ruby),
  lua: legacy(lua),
  swift: legacy(swift),
  perl: legacy(perl),
  r: legacy(r),
  haskell: legacy(haskell),
  protobuf: legacy(protobuf),
  cmake: legacy(cmake),
  groovy: legacy(groovy),
  csharp: legacy(csharp),
  kotlin: legacy(kotlin),
  scala: legacy(scala),
  dart: legacy(dart),
};

function languageFor(name) {
  const lang = languageId(name);
  return lang ? LANGUAGES[lang]() : [];
}

// ── Resaltado del diff ──────────────────────────────────────────────────────

/**
 * Misma paleta que `oneDark` (el tema del editor), pero con clases `hl-*` que pinta
 * ui/style.css: el diff no es una EditorView y no monta los estilos del tema.
 */
const diffHighlighter = tagHighlighter([
  { tag: t.keyword, class: 'hl-violet' },
  { tag: [t.name, t.deleted, t.character, t.propertyName, t.macroName], class: 'hl-coral' },
  { tag: [t.function(t.variableName), t.labelName], class: 'hl-malibu' },
  { tag: [t.color, t.constant(t.name), t.standard(t.name)], class: 'hl-whiskey' },
  { tag: [t.definition(t.name), t.separator], class: 'hl-ivory' },
  {
    tag: [t.typeName, t.className, t.number, t.changed, t.annotation, t.modifier, t.self, t.namespace],
    class: 'hl-chalky',
  },
  {
    tag: [t.operator, t.operatorKeyword, t.url, t.escape, t.regexp, t.special(t.string)],
    class: 'hl-cyan',
  },
  { tag: [t.meta, t.comment], class: 'hl-stone' },
  { tag: t.strong, class: 'hl-strong' },
  { tag: t.emphasis, class: 'hl-emphasis' },
  { tag: t.strikethrough, class: 'hl-strike' },
  { tag: t.link, class: 'hl-stone hl-link' },
  { tag: t.heading, class: 'hl-coral hl-strong' },
  { tag: [t.atom, t.bool, t.special(t.variableName)], class: 'hl-whiskey' },
  { tag: [t.processingInstruction, t.string, t.inserted], class: 'hl-sage' },
  { tag: t.invalid, class: 'hl-invalid' },
]);

/**
 * Resalta `text` como el archivo `name`. Devuelve una entrada por línea, cada una
 * una lista de tramos `[texto, clases]` (`clases` vacío = sin color).
 */
function highlightText(name, text) {
  const lines = text.split('\n');
  const plain = () => lines.map((l) => [[l, '']]);
  const lang = languageFor(name);
  if (Array.isArray(lang)) return plain();
  const state = EditorState.create({ doc: text, extensions: [lang] });
  // Tiempo acotado: un hunk enorme se queda sin color antes que congelar la UI.
  const tree = ensureSyntaxTree(state, state.doc.length, 100);
  if (!tree) return plain();

  const out = [[]];
  const emit = (from, to, cls) => {
    if (to <= from) return;
    const parts = text.slice(from, to).split('\n');
    parts.forEach((part, i) => {
      if (i > 0) out.push([]);
      if (part) out[out.length - 1].push([part, cls]);
    });
  };
  let pos = 0;
  highlightTree(tree, diffHighlighter, (from, to, cls) => {
    emit(pos, from, '');
    emit(from, to, cls);
    pos = to;
  });
  emit(pos, text.length, '');
  while (out.length < lines.length) out.push([]);
  return out;
}

/**
 * Colores de sintaxis para las líneas de un hunk. `kinds` tiene un carácter por
 * línea (`+` añadida, `-` borrada, ` ` contexto). Se parsean por separado la versión
 * vieja (contexto + borradas) y la nueva (contexto + añadidas) para que mezclarlas
 * no rompa el análisis (p. ej. un string que se abre en una y se cierra en otra).
 * Devuelve, por línea, la lista de tramos `[texto, clases]`.
 */
export function highlightHunk(name, kinds, texts) {
  const oldLines = [];
  const newLines = [];
  texts.forEach((text, i) => {
    if (kinds[i] !== '+') oldLines.push(text);
    if (kinds[i] !== '-') newLines.push(text);
  });
  const oldHl = highlightText(name, oldLines.join('\n'));
  const newHl = highlightText(name, newLines.join('\n'));
  let o = 0;
  let n = 0;
  return texts.map((_, i) => {
    switch (kinds[i]) {
      case '+':
        return newHl[n++];
      case '-':
        return oldHl[o++];
      default:
        o++;
        return newHl[n++];
    }
  });
}

/**
 * Editor con varios documentos abiertos (pestañas). Cada documento guarda su propio
 * `EditorState` (texto, cursor, historial de deshacer) y su texto "limpio"; una sola
 * `EditorView` muestra el activo. `onSave(text)` es Ctrl+S sobre el activo y
 * `onDirty(key, dirty)` avisa cuando un documento pasa a tener o no cambios sin guardar.
 */
export function createEditor(el, onSave, onDirty) {
  /** key → { state, clean, dirty } */
  const docs = new Map();
  let active = null;

  const isDirty = (doc) => doc.state.doc.toString() !== doc.clean;
  const notify = (key, doc) => {
    const dirty = isDirty(doc);
    if (dirty !== doc.dirty) {
      doc.dirty = dirty;
      onDirty(key, dirty);
    }
  };

  const extensions = (name) => [
    Prec.high(
      keymap.of([
        {
          key: 'Mod-s',
          preventDefault: true,
          run: (view) => {
            onSave(view.state.doc.toString());
            return true;
          },
        },
      ]),
    ),
    basicSetup,
    oneDark,
    languageFor(name),
    EditorView.updateListener.of((update) => {
      const doc = active && docs.get(active);
      if (!doc || !update.docChanged) return;
      doc.state = update.state;
      notify(active, doc);
    }),
    EditorView.theme({ '&': { height: '100%' }, '.cm-scroller': { overflow: 'auto' } }),
  ];

  const view = new EditorView({
    parent: el,
    state: EditorState.create({ doc: '', extensions: extensions('') }),
  });

  /** Guarda el estado de la vista (cursor, scroll de selección) en el documento activo. */
  const stash = () => {
    const doc = active && docs.get(active);
    if (doc) doc.state = view.state;
  };

  const blank = () => {
    active = null;
    view.setState(EditorState.create({ doc: '', extensions: extensions('') }));
  };

  return {
    hasDoc(key) {
      return docs.has(key);
    },
    /** Abre (o reemplaza) un documento con texto recién leído de disco y lo muestra. */
    openDoc(key, text, name) {
      stash();
      const doc = { state: EditorState.create({ doc: text, extensions: extensions(name) }), clean: text, dirty: false };
      const prev = docs.get(key);
      docs.set(key, doc);
      if (prev?.dirty) onDirty(key, false);
      active = key;
      view.setState(doc.state);
    },
    /** Muestra un documento ya abierto conservando su historial. */
    showDoc(key) {
      const doc = docs.get(key);
      if (!doc || key === active) return;
      stash();
      active = key;
      view.setState(doc.state);
    },
    /** Cambió en disco y no tiene cambios sin guardar: reemplaza su texto. */
    reloadDoc(key, text) {
      const doc = docs.get(key);
      if (!doc) return;
      doc.clean = text; // antes de despachar: el updateListener compara contra él
      const current = key === active ? view.state : doc.state;
      if (current.doc.toString() === text) {
        // Nuestro propio guardado (o nada cambió): no mover el cursor.
        notify(key, doc);
        return;
      }
      if (key === active) {
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
        doc.state = view.state;
      } else {
        doc.state = doc.state.update({ changes: { from: 0, to: doc.state.doc.length, insert: text } }).state;
      }
      notify(key, doc);
    },
    closeDoc(key) {
      docs.delete(key);
      if (key === active) blank();
    },
    /** Sin documento activo (p. ej. archivo binario): vista vacía. */
    clear() {
      stash();
      blank();
    },
    markSaved(key, text) {
      const doc = docs.get(key);
      if (!doc) return;
      doc.clean = text;
      notify(key, doc);
    },
    getText(key) {
      const doc = docs.get(key);
      if (!doc) return null;
      return key === active ? view.state.doc.toString() : doc.state.doc.toString();
    },
    focus() {
      view.focus();
    },
    destroy() {
      docs.clear();
      view.destroy();
    },
  };
}
