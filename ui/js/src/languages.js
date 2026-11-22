// Qué lenguaje del editor le toca a un archivo, por nombre. Lógica pura (sin
// CodeMirror) para poder probarla con `node --test`; index.js mapea el id a la
// extensión de CodeMirror.

/** Extensión (sin punto, en minúsculas) → lenguaje. */
export const EXTENSIONS = {
  rs: 'rust',
  js: 'javascript', mjs: 'javascript', cjs: 'javascript',
  jsx: 'jsx',
  ts: 'typescript', mts: 'typescript', cts: 'typescript',
  tsx: 'tsx',
  json: 'json', jsonc: 'json', json5: 'json', jsonl: 'json', ndjson: 'json',
  webmanifest: 'json', har: 'json', ipynb: 'json',
  md: 'markdown', markdown: 'markdown', mdx: 'markdown',
  py: 'python', pyw: 'python', pyi: 'python',
  html: 'html', htm: 'html', xhtml: 'html', vue: 'html', svelte: 'html',
  css: 'css', scss: 'scss', sass: 'sass', less: 'less',
  yml: 'yaml', yaml: 'yaml',
  xml: 'xml', svg: 'xml', xsd: 'xml', xsl: 'xml', xslt: 'xml', plist: 'xml',
  csproj: 'xml', fsproj: 'xml', vbproj: 'xml', props: 'xml', targets: 'xml',
  resx: 'xml', xaml: 'xml', wsdl: 'xml', rss: 'xml', atom: 'xml',
  sql: 'sql',
  java: 'java',
  c: 'cpp', h: 'cpp', cpp: 'cpp', cc: 'cpp', cxx: 'cpp', hpp: 'cpp', hh: 'cpp', hxx: 'cpp',
  ino: 'cpp',
  go: 'go',
  php: 'php',
  sh: 'shell', bash: 'shell', zsh: 'shell', ksh: 'shell', fish: 'shell', command: 'shell',
  dockerfile: 'dockerfile', containerfile: 'dockerfile',
  toml: 'toml',
  env: 'properties', ini: 'properties', cfg: 'properties', conf: 'properties',
  properties: 'properties', editorconfig: 'properties', gitconfig: 'properties',
  ps1: 'powershell', psm1: 'powershell', psd1: 'powershell',
  diff: 'diff', patch: 'diff',
  rb: 'ruby', rake: 'ruby', gemspec: 'ruby',
  lua: 'lua',
  swift: 'swift',
  pl: 'perl', pm: 'perl',
  r: 'r',
  hs: 'haskell',
  proto: 'protobuf',
  cmake: 'cmake',
  groovy: 'groovy', gradle: 'groovy',
  cs: 'csharp',
  kt: 'kotlin', kts: 'kotlin',
  scala: 'scala', sc: 'scala',
  dart: 'dart',
};

/** Nombres exactos (en minúsculas) sin extensión significativa. */
export const FILE_NAMES = {
  dockerfile: 'dockerfile',
  containerfile: 'dockerfile',
  makefile: 'shell',
  gnumakefile: 'shell',
  'cmakelists.txt': 'cmake',
  gemfile: 'ruby',
  rakefile: 'ruby',
  vagrantfile: 'ruby',
  jenkinsfile: 'groovy',
  'nginx.conf': 'nginx',
  '.gitignore': 'shell',
  '.dockerignore': 'shell',
  '.npmignore': 'shell',
  '.gitattributes': 'properties',
  '.gitmodules': 'properties',
  '.npmrc': 'properties',
  '.bashrc': 'shell',
  '.bash_profile': 'shell',
  '.profile': 'shell',
  '.zshrc': 'shell',
  '.babelrc': 'json',
  '.eslintrc': 'json',
  '.prettierrc': 'json',
  'cargo.lock': 'toml',
  'pipfile': 'toml',
  'poetry.lock': 'toml',
};

/**
 * Lenguaje para un nombre de archivo. Igual que los iconos (ui/src/icons.rs): nombre
 * exacto y luego la extensión más larga primero. Además reconoce variantes habituales:
 * `Dockerfile.dev`, `api.Dockerfile`, `.env.local`, `.env.production`.
 */
export function languageId(path) {
  const name = (path.split(/[\\/]/).pop() || '').toLowerCase();
  if (FILE_NAMES[name]) return FILE_NAMES[name];
  if (name.startsWith('dockerfile.') || name.startsWith('containerfile.')) return 'dockerfile';
  if (name === '.env' || name.startsWith('.env.')) return 'properties';
  for (let i = name.indexOf('.', 1); i !== -1; i = name.indexOf('.', i + 1)) {
    const lang = EXTENSIONS[name.slice(i + 1)];
    if (lang) return lang;
  }
  // Dotfile sin más puntos (`.editorconfig`).
  if (name.startsWith('.') && EXTENSIONS[name.slice(1)]) return EXTENSIONS[name.slice(1)];
  return null;
}
