// Component smoke tests: real Edge/DOM, explicitly mocked Tauri IPC.
// Backend tests separately launch the real official Codex executable.
import { chromium } from '@playwright/test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const server = spawn(process.execPath, [resolve(root, 'node_modules/vite/bin/vite.js'), '--host', '127.0.0.1', '--port', '1421'], { cwd: root, stdio: 'ignore' });
let browser;
try {
  let ready = false;
  for (let i = 0; i < 60; i++) {
    try { ready = (await fetch('http://127.0.0.1:1421/settings.html')).ok; } catch {}
    if (ready) break;
    await new Promise(r => setTimeout(r, 100));
  }
  assert(ready, 'Vite test server did not become ready');
  browser = await chromium.launch({ channel: 'msedge', headless: true });
  const page = await browser.newPage();
  await page.addInitScript(() => {
    window.testCalls = [];
    window.testConnected = false;
    window.testPending = false;
    window.testChatError = false;
    window.__TAURI_INTERNALS__ = {
      transformCallback: () => 1,
      invoke: async (cmd, args) => {
        window.testCalls.push({ cmd, args });
        if (cmd === 'boot') return { settings: { chatProvider: 'anthropic' }, version: 'test' };
        if (cmd === 'hooks_status') return { installed: false, settingsPath: '', hookPath: '', hookReady: false };
        if (cmd === 'secret_present') return false;
        if (cmd === 'chatgpt_status') return { connected: window.testConnected, pending: window.testPending, message: window.testConnected ? 'Fixture: connected' : window.testPending ? 'Fixture: login pending' : 'Fixture: disconnected' };
        if (cmd === 'chatgpt_connect') { window.testPending = true; return; }
        if (cmd === 'chatgpt_disconnect') { window.testPending = false; window.testConnected = false; return; }
        if (cmd === 'chat_send') {
          if (window.testChatError) throw 'Fixture chat error';
          return { text: 'Fixture reply' };
        }
        return null;
      },
    };
  });
  await page.goto('http://127.0.0.1:1421/settings.html');
  const section = page.locator('section').filter({ has: page.getByRole('heading', { name: 'Chat provider', exact: true }) });
  await section.getByRole('combobox').selectOption('chatgpt');
  assert(await page.evaluate(() => window.testCalls.some(c => c.cmd === 'save_settings' && c.args.settings.chatProvider === 'chatgpt')));
  await section.getByRole('button', { name: 'Connect ChatGPT', exact: true }).click();
  await section.getByText('Fixture: login pending').waitFor();
  await page.evaluate(() => { window.testConnected = true; window.testPending = false; });
  await section.getByRole('button', { name: 'Check status' }).click();
  await section.getByText('Fixture: connected').waitFor();
  await section.getByRole('button', { name: 'Disconnect / cancel login' }).click();
  await section.getByText('Fixture: disconnected').waitFor();
  console.log('PASS provider selection, OAuth connect IPC, status polling, disconnect IPC');
  await page.evaluate(async () => {
    const { State } = await import('/src/core/state.ts');
    const { buildPrompt } = await import('/src/views/chat.ts');
    document.body.replaceChildren();
    State.settings.chatProvider = 'chatgpt';
    const view = buildPrompt(() => {});
    document.body.append(view.el);
    State.subscribe(() => view.sync());
    view.sync();
  });
  await page.getByPlaceholder('Ask me anything…').fill('Hello');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await page.getByText('Fixture reply', { exact: true }).waitFor();
  assert.equal(await page.locator('.chat-row').count(), 2);
  await page.getByRole('button', { name: 'New chat', exact: true }).click({ timeout: 3000 });
  assert.equal(await page.locator('.chat-row').count(), 0);
  assert(await page.evaluate(() => window.testCalls.some(c => c.cmd === 'chat_reset')));
  await page.evaluate(() => { window.testChatError = true; });
  await page.getByPlaceholder('Ask me anything…').fill('Retry me');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await page.waitForFunction(() => document.querySelector('.chat-input')?.value === 'Retry me');
  assert.equal(await page.locator('.chat-row').count(), 0);
  console.log('PASS rendered reply, New chat reset, failed-turn rollback and retry input');
} finally {
  await browser?.close();
  server.kill();
}
