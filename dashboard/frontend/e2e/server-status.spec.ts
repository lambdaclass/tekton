import { test, expect } from './fixtures';

const GiB = 1024 ** 3;

// Prometheus isn't available in tests (playwright.config points PROMETHEUS_URL at a
// closed port), so data-driven tests stub the backend response.
function stubMetrics(range = '24h') {
  const end = 1_760_000_000;
  const step = 360;
  const start = end - 240 * step;
  const series = (f: (i: number) => number) =>
    Array.from({ length: 241 }, (_, i) => [start + i * step, f(i)]);
  return {
    range,
    start,
    end,
    step,
    memory: { used: series((i) => 16 * GiB + (i % 10) * 0.1 * GiB), total: 64 * GiB },
    disk: { used: series(() => 450 * GiB), total: 500 * GiB },
    network: { rx: series(() => 1_250_000), tx: series(() => 250_000) },
  };
}

test.describe('Server Status page', () => {
  test('renders heading and range selector', async ({ adminPage: page }) => {
    await page.goto('/server');

    await expect(page.getByRole('heading', { name: 'Server Status' })).toBeVisible();
    await expect(page.getByRole('combobox')).toHaveText('Last 24 hours');
  });

  test('shows a friendly error when Prometheus is unreachable', async ({ adminPage: page }) => {
    await page.goto('/server');

    await expect(page.getByText('Prometheus is not reachable')).toBeVisible();
  });

  test('renders usage cards and charts from metrics', async ({ adminPage: page }) => {
    await page.route('**/api/admin/server-metrics*', (route) =>
      route.fulfill({ json: stubMetrics() }),
    );
    await page.goto('/server');

    // Last memory sample: 16 GiB of 64 GiB → 25%; disk 450/500 → 90%.
    await expect(page.getByText('25%')).toBeVisible();
    await expect(page.getByText('90%')).toBeVisible();
    await expect(page.getByText('450 GiB of 500 GiB')).toBeVisible();
    // 1.25 MB/s in, 250 KB/s out, shown as bit rates.
    await expect(page.getByText('10.0 Mbps').first()).toBeVisible();
    await expect(page.getByText('2.0 Mbps').first()).toBeVisible();

    await expect(page.getByText('Network traffic')).toBeVisible();
    await expect(page.getByText('Total 64.0 GiB')).toBeVisible();
    await expect(page.getByText('Total 500 GiB')).toBeVisible();
  });

  test('changing the range refetches with the new range', async ({ adminPage: page }) => {
    const ranges: string[] = [];
    await page.route('**/api/admin/server-metrics*', (route) => {
      const range = new URL(route.request().url()).searchParams.get('range') ?? '';
      ranges.push(range);
      return route.fulfill({ json: stubMetrics(range) });
    });
    await page.goto('/server');
    await expect(page.getByText('90%')).toBeVisible();

    await page.getByRole('combobox').click();
    await page.getByRole('option', { name: 'Last 7 days' }).click();

    await expect.poll(() => ranges).toContain('7d');
  });

  test('member is redirected away and API is forbidden', async ({ memberPage: page }) => {
    await page.goto('/server');
    await expect(page).toHaveURL('/');

    const res = await page.request.get('/api/admin/server-metrics');
    expect(res.status()).toBe(403);
  });
});
