import assert from 'node:assert/strict';
import { createReadStream, existsSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { chromium } from 'playwright-core';

const directory = process.argv[2];
const server = createServer((request, response) => {
  const pathname = new URL(request.url, 'http://127.0.0.1').pathname;
  const path = join(directory, pathname === '/' ? 'index.html' : pathname);
  const type = path.endsWith('.js') ? 'text/javascript' : path.endsWith('.css') ? 'text/css' : 'text/html';
  if (!existsSync(path)) {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { 'Content-Type': type });
  createReadStream(path).pipe(response);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));

const browser = await chromium.launch({
  executablePath: process.env.FRILADD_BROWSER,
  headless: true,
  args: ['--no-sandbox'],
});
try {
  const page = await browser.newPage({ reducedMotion: 'reduce' });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const envelope = page.locator('[data-component="envelope-reveal"]');
  const letter = page.locator('[role="region"][aria-label="Letter contents"]');
  const button = page.getByRole('button', { name: 'Open the letter' });
  await button.waitFor();
  assert.equal(await envelope.getAttribute('data-state'), 'closed');
  assert.equal(await button.getAttribute('aria-expanded'), 'false');
  assert.equal(await letter.isVisible(), false);

  await button.focus();
  await page.keyboard.press('Enter');
  const close = page.getByRole('button', { name: 'Close the letter' });
  await close.waitFor();
  assert.equal(await envelope.getAttribute('data-state'), 'open');
  assert.equal(await close.getAttribute('aria-expanded'), 'true');
  assert.equal(await letter.isVisible(), true);
  assert.match(await letter.textContent(), /Production verification.*Pinned source/);
  assert.equal(await letter.evaluate((node) => getComputedStyle(node).position), 'absolute');

  await close.click();
  await button.waitFor();
  assert.equal(await envelope.getAttribute('data-state'), 'closed');
  assert.equal(await letter.isVisible(), false);
  assert.deepEqual(errors, []);
  console.log(`Chromium ${browser.version()}: keyboard reveal, click to close, accessibility state and Tailwind styling passed`);
} finally {
  await browser.close();
  server.close();
}
