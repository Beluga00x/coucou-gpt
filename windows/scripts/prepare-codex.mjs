// Stage the exact npm-pinned official native CLI. No global Node/PATH required at runtime.
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cpSync, mkdirSync, readFileSync } from 'node:fs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('ChatGPT bundle currently requires Windows x64');
const pkg = dirname(require.resolve('@openai/codex-win32-x64/package.json'));
const src = join(pkg, 'vendor', 'x86_64-pc-windows-msvc', 'bin');
const dest = join(root, 'src-tauri', 'resources', 'codex');
mkdirSync(dest, { recursive: true });
cpSync(src, dest, { recursive: true });
const cli = dirname(require.resolve('@openai/codex/package.json'));
cpSync(join(cli, 'README.md'), join(dest, 'README.md'));
cpSync(join(root, 'third-party', 'codex-LICENSE'), join(dest, 'LICENSE'));
cpSync(join(root, 'third-party', 'codex-NOTICE'), join(dest, 'NOTICE'));
console.log('Staged official Codex', JSON.parse(readFileSync(join(cli, 'package.json'), 'utf8')).version, 'at', dest);
