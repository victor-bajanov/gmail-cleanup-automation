import puppeteer, { type Browser, type Page } from 'puppeteer';

const VITE_URL = 'http://localhost:5173';

let browser: Browser;

export async function setupBrowser(): Promise<Browser> {
  browser = await puppeteer.launch({ headless: true });
  return browser;
}

export async function teardownBrowser(): Promise<void> {
  if (browser) await browser.close();
}

export async function newPage(): Promise<Page> {
  const page = await browser.newPage();
  return page;
}

/**
 * Navigate to the app with mock Tauri internals injected BEFORE the app JS loads.
 */
export async function navigateWithMock(
  page: Page,
  mockScript: string
): Promise<void> {
  await page.evaluateOnNewDocument(mockScript);
  await page.goto(VITE_URL, { waitUntil: 'networkidle0' });
}

/**
 * Wait for a selector and click it.
 */
export async function clickAndWait(
  page: Page,
  selector: string,
  waitMs = 500
): Promise<void> {
  await page.waitForSelector(selector, { timeout: 5000 });
  await page.click(selector);
  await new Promise(r => setTimeout(r, waitMs));
}
