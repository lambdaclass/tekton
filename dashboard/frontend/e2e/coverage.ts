import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import { Page } from '@playwright/test';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const NYC_OUTPUT = path.resolve(__dirname, '..', '.nyc_output');

export async function collectCoverage(page: Page): Promise<void> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const coverage = await page.evaluate(() => (window as any).__coverage__);
  if (!coverage) return;

  fs.mkdirSync(NYC_OUTPUT, { recursive: true });
  const id = crypto.randomUUID();
  fs.writeFileSync(
    path.join(NYC_OUTPUT, `coverage-${id}.json`),
    JSON.stringify(coverage),
  );
}
