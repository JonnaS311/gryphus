// Genera ui/icons/ a partir de `material-icon-theme` (Material Icon Theme, MIT):
//   - los SVG que usa el tema oscuro,
//   - manifest.json con las tablas de búsqueda que consume ui/src/icons.rs,
//   - LICENSE del paquete.
// Uso: `npm run icons` (en ui/js). El resultado se versiona: compilar no requiere Node.

import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(here, '../node_modules/material-icon-theme');
const out = resolve(here, '../../icons');
const manifest = JSON.parse(readFileSync(join(pkg, 'dist/material-icons.json'), 'utf8'));

// VS Code asigna muchos iconos por "language id" (rust, python…), no por extensión.
// Gryphus no tiene language ids: se traducen a las extensiones habituales.
const LANGUAGE_EXTENSIONS = {
  rust: ['rs'],
  javascript: ['js', 'mjs', 'cjs'],
  javascriptreact: ['jsx'],
  typescript: ['ts', 'mts', 'cts'],
  typescriptreact: ['tsx'],
  python: ['py', 'pyw', 'pyi'],
  json: ['json'],
  jsonc: ['jsonc'],
  json5: ['json5'],
  jsonl: ['jsonl'],
  markdown: ['md', 'markdown'],
  html: ['html', 'htm'],
  css: ['css'],
  scss: ['scss'],
  sass: ['sass'],
  less: ['less'],
  yaml: ['yml', 'yaml'],
  toml: ['toml'],
  xml: ['xml'],
  java: ['java'],
  go: ['go'],
  c: ['c', 'h'],
  cpp: ['cpp', 'cc', 'cxx', 'hpp', 'hh', 'hxx'],
  csharp: ['cs'],
  fsharp: ['fs', 'fsx'],
  ruby: ['rb'],
  php: ['php'],
  shellscript: ['sh', 'bash', 'zsh', 'fish'],
  powershell: ['ps1', 'psm1', 'psd1'],
  bat: ['bat', 'cmd'],
  sql: ['sql'],
  kotlin: ['kt', 'kts'],
  swift: ['swift'],
  lua: ['lua'],
  dart: ['dart'],
  vue: ['vue'],
  svelte: ['svelte'],
  r: ['r'],
  perl: ['pl', 'pm'],
  diff: ['diff', 'patch'],
  ini: ['ini'],
  properties: ['properties'],
  scala: ['scala'],
  elixir: ['ex', 'exs'],
  haskell: ['hs'],
  clojure: ['clj', 'cljs', 'cljc'],
  erlang: ['erl'],
  julia: ['jl'],
  zig: ['zig'],
  nim: ['nim'],
  graphql: ['graphql', 'gql'],
  terraform: ['tf', 'tfvars'],
  hcl: ['hcl'],
  proto: ['proto'],
  prisma: ['prisma'],
  latex: ['tex'],
  plaintext: ['txt'],
  log: ['log'],
  csv: ['csv'],
};

const icons = new Set();
const iconName = (id) => {
  const def = manifest.iconDefinitions[id];
  if (!def) return null;
  const name = basename(def.iconPath, '.svg');
  icons.add(name);
  return name;
};

const table = (source) => {
  const result = {};
  for (const [key, id] of Object.entries(source)) {
    const name = iconName(id);
    if (name) result[key.toLowerCase()] = name;
  }
  return result;
};

const fileExtensions = table(manifest.fileExtensions);
for (const [lang, exts] of Object.entries(LANGUAGE_EXTENSIONS)) {
  const id = manifest.languageIds[lang];
  const name = id && iconName(id);
  if (!name) continue;
  for (const ext of exts) fileExtensions[ext] ??= name;
}

const folderNames = table(manifest.folderNames);
const folderNamesExpanded = table(manifest.folderNamesExpanded);
// Si la versión abierta siempre es `<icono>-open`, no hace falta guardar la tabla.
const expandedIsSuffix = Object.entries(folderNames).every(
  ([k, v]) => folderNamesExpanded[k] === `${v}-open`,
);

const data = {
  file: iconName(manifest.file),
  folder: iconName(manifest.folder),
  folderExpanded: iconName(manifest.folderExpanded),
  fileNames: table(manifest.fileNames),
  fileExtensions,
  folderNames,
  ...(expandedIsSuffix ? {} : { folderNamesExpanded }),
};

if (existsSync(out)) rmSync(out, { recursive: true });
mkdirSync(out, { recursive: true });
for (const name of icons) {
  cpSync(join(pkg, 'icons', `${name}.svg`), join(out, `${name}.svg`));
}
writeFileSync(join(out, 'manifest.json'), JSON.stringify(data));
cpSync(join(pkg, 'LICENSE'), join(out, 'LICENSE'));
const version = JSON.parse(readFileSync(join(pkg, 'package.json'), 'utf8')).version;
console.log(`material-icon-theme ${version}: ${icons.size} iconos → ${out}`);
