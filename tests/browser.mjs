// Headless Chromium for the browser tests (puppeteer-core: `npm ci --prefix tests`; CHROMIUM
// names the browser binary).
import puppeteer from 'puppeteer-core';

export function launch() {
  return puppeteer.launch({
    executablePath: process.env.CHROMIUM || '/usr/sbin/chromium',
    headless: true,
    args: ['--disable-gpu']
  });
}
