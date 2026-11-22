// Resaltado del editor: qué lenguaje le toca a cada nombre de archivo.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { languageId, EXTENSIONS, FILE_NAMES } from '../src/languages.js';

test('extensiones habituales', () => {
  assert.equal(languageId('src/main.rs'), 'rust');
  assert.equal(languageId('docker-compose.yml'), 'yaml');
  assert.equal(languageId('ci.YAML'), 'yaml');
  assert.equal(languageId('package.json'), 'json');
  assert.equal(languageId('tsconfig.jsonc'), 'json');
  assert.equal(languageId('Cargo.toml'), 'toml');
  assert.equal(languageId('pom.xml'), 'xml');
  assert.equal(languageId('schema.sql'), 'sql');
  assert.equal(languageId('build.sh'), 'shell');
  assert.equal(languageId('setup.ps1'), 'powershell');
  assert.equal(languageId('main.go'), 'go');
  assert.equal(languageId('Program.cs'), 'csharp');
});

test('la extensión más larga primero', () => {
  assert.equal(languageId('app.spec.ts'), 'typescript');
  assert.equal(languageId('C:\\repo\\ui\\index.d.ts'), 'typescript');
});

test('Dockerfile y variantes', () => {
  assert.equal(languageId('Dockerfile'), 'dockerfile');
  assert.equal(languageId('docker/Dockerfile.dev'), 'dockerfile');
  assert.equal(languageId('api.dockerfile'), 'dockerfile');
  assert.equal(languageId('Containerfile'), 'dockerfile');
});

test('.env y otros dotfiles', () => {
  assert.equal(languageId('.env'), 'properties');
  assert.equal(languageId('.env.local'), 'properties');
  assert.equal(languageId('.env.production'), 'properties');
  assert.equal(languageId('config.env'), 'properties');
  assert.equal(languageId('.editorconfig'), 'properties');
  assert.equal(languageId('.gitignore'), 'shell');
  assert.equal(languageId('Makefile'), 'shell');
});

test('desconocidos sin resaltado', () => {
  assert.equal(languageId('LICENSE'), null);
  assert.equal(languageId('x.unknown'), null);
  assert.equal(languageId(''), null);
});

test('todo lenguaje mapeado tiene fábrica en index.js', () => {
  const index = readFileSync(new URL('../src/index.js', import.meta.url), 'utf8');
  const body = index.slice(index.indexOf('const LANGUAGES = {'));
  const defined = new Set([...body.matchAll(/^ {2}(\w+):/gm)].map((m) => m[1]));
  for (const lang of [...Object.values(EXTENSIONS), ...Object.values(FILE_NAMES)]) {
    assert.ok(defined.has(lang), `falta la fábrica de ${lang}`);
  }
});
