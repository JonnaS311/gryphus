// Pegamento JS de Gryphus. Expone `window.GryphusJs` con dos fábricas:
//   createTerminal(el, onData, onResize) y createEditor(el, onSave, onDirty).
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

import { decode } from './stream.js';

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

export function createTerminal(el, onData, onResize) {
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

  return {
    /** Canal de Tauri cuya salida se escribe directamente en la terminal (sin pasar por WASM). */
    channel() {
      const ch = new window.__TAURI__.core.Channel();
      ch.onmessage = handle;
      return ch;
    },
    reset() {
      term.reset();
    },
    /** Ajusta al contenedor e informa el tamaño actual. */
    fit() {
      refit();
      onResize(term.cols, term.rows);
    },
    focus() {
      term.focus();
    },
    write(text) {
      term.write(text);
    },
    dispose() {
      window.removeEventListener('gryphus:fonts', onFonts);
      observer.disconnect();
      term.dispose();
    },
  };
}

// ── Editor ──────────────────────────────────────────────────────────────────

const LANGS = {
  rs: () => rust(),
  js: () => javascript(),
  mjs: () => javascript(),
  cjs: () => javascript(),
  jsx: () => javascript({ jsx: true }),
  ts: () => javascript({ typescript: true }),
  tsx: () => javascript({ typescript: true, jsx: true }),
  json: () => json(),
  md: () => markdown(),
  py: () => python(),
  html: () => html(),
  htm: () => html(),
  css: () => css(),
};

function languageFor(name) {
  const ext = (name.split('.').pop() || '').toLowerCase();
  const make = LANGS[ext];
  return make ? make() : [];
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
