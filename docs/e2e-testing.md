# E2E Testing Guide

End-to-end tests live in `dashboard/frontend/e2e/` and run with [Playwright](https://playwright.dev/). They exercise the full stack — PostgreSQL, Rust backend, and React frontend — through a real Chromium browser.

---

## Quick Start

### Prerequisites

| Tool       | Version | Notes                            |
| ---------- | ------- | -------------------------------- |
| Docker     | any     | For the local PostgreSQL container |
| Rust       | stable  | Backend compilation              |
| Node.js    | 22+     | Frontend tooling                 |
| PostgreSQL | 16      | Via Docker (managed by `make`)   |

### First-time setup

```bash
# 1. Install dependencies and start the local PostgreSQL container
make deps

# 2. Create the test database (make deps creates "dashboard", not "tekton_test")
docker exec -i tekton-postgres psql -U tekton -d postgres -c "CREATE DATABASE tekton_test;"

# 3. Install Playwright browsers
cd dashboard/frontend
npx playwright install --with-deps chromium
```

### Running tests

```bash
cd dashboard/frontend

# Set DATABASE_URL for the Docker-based PostgreSQL (the default in playwright.config.ts
# uses the OS user, which won't match the Docker container credentials)
export DATABASE_URL="postgres://tekton:tekton@localhost:5432/tekton_test"

# Run the full suite (Playwright auto-starts the backend via its webServer config)
npm run test:e2e

# Interactive UI mode — great for debugging
npm run test:e2e:ui

# Single file
npx playwright test e2e/tasks-list.spec.ts

# View the HTML report after a run
npx playwright show-report
```

> **Tip:** The Playwright `webServer` config runs `cargo run --release` on port 3200 and reuses an already-running server if one is detected.

---

## Architecture & Project Layout

```
dashboard/frontend/e2e/
├── fixtures.ts              # Custom test fixtures (role pages, TEST_IDS, coverage)
├── global-setup.ts          # Seed DB, generate JWTs, save auth storage states
├── global-teardown.ts       # Drop all tables, remove .auth/ directory
├── coverage.ts              # Collect window.__coverage__ from instrumented builds
├── seed.sql                 # Full seed data (users, tasks, policies, intake, etc.)
│
├── activity-sidebar.spec.ts
├── admin-intake.spec.ts
├── admin-policies.spec.ts
├── admin-secrets.spec.ts
├── admin-users.spec.ts
├── audit-log.spec.ts
├── auth.spec.ts
├── command-palette.spec.ts
├── cost-dashboard.spec.ts
├── intake-board.spec.ts
├── navigation.spec.ts
├── preview-detail.spec.ts
├── previews.spec.ts
├── responsive.spec.ts
├── settings.spec.ts
├── task-chat.spec.ts
├── task-create.spec.ts
├── task-detail.spec.ts
├── tasks-list.spec.ts
├── theme-toggle.spec.ts
│
└── lighthouse/
    ├── lighthouse-config.ts  # Thresholds, Chrome launcher, audit runner
    └── lighthouse.spec.ts    # Performance/a11y audits for key pages
```

### Test lifecycle

```
global-setup.ts                      global-teardown.ts
  ┌─────────────────────┐              ┌──────────────────────┐
  │ 1. Run seed.sql      │              │ 1. DROP all tables    │
  │ 2. Generate JWTs     │  ──tests──▶  │ 2. Remove .auth/      │
  │ 3. Save auth states  │              │ 3. Clean .nyc_output  │
  └─────────────────────┘              └──────────────────────┘
```

1. **global-setup** seeds the `tekton_test` database via `psql`, generates HS256 JWTs for three test users (admin, member, viewer), and writes Playwright storage states to `.auth/*.json` with `dashboard_session` cookies.
2. **Tests run fully parallel** across spec files. Each spec uses role-based fixtures that load the correct auth state.
3. **global-teardown** drops all tables and removes `.auth/` storage files.

---

## Test Patterns & Conventions

### No mocks

Tests hit the real stack: a real PostgreSQL database, the real Rust backend, and a real Chromium browser. **Do not mock the database, API responses, or any backend behavior.** The goal is to catch the bugs that only surface when every layer is wired together. If a test is hard to write without mocking, that's a signal the feature needs better seed data or a test-specific API state — not a mock.

### Fixtures

Always import from `./fixtures`, not from `@playwright/test` directly:

```ts
import { test, expect, TEST_IDS } from './fixtures';
```

The custom fixtures provide role-based browser contexts with pre-loaded auth:

| Fixture           | Role     | Storage State         |
| ----------------- | -------- | --------------------- |
| `authenticatedPage` | admin  | `.auth/admin.json`    |
| `adminPage`       | admin    | `.auth/admin.json`    |
| `memberPage`      | member   | `.auth/member.json`   |
| `viewerPage`      | viewer   | `.auth/viewer.json`   |

Every page fixture automatically calls `collectCoverage()` after the test completes, so code coverage collection requires no manual effort.

### TEST_IDS

`TEST_IDS` is a constant map of identifiers that correspond to entities in `seed.sql`. Use these instead of hardcoded strings:

```ts
// Good
await page.goto(`/tasks/${TEST_IDS.tasks.completed}`);

// Bad — breaks if seed data changes
await page.goto('/tasks/task-completed-1');
```

### Selectors

Prefer accessibility-first selectors:

```ts
// Preferred
page.getByRole('button', { name: 'Create Task' })
page.getByLabel('Task title')
page.getByText('Successfully created')

// Avoid
page.locator('.btn-primary')
page.locator('#task-title-input')
```

### Serial vs parallel

Use `test.describe.serial` only for CRUD flows where test order matters (create -> verify -> delete). Default to `test.describe` for everything else — tests run fully parallel.

```ts
// Use serial when tests depend on prior test state
test.describe.serial('Task CRUD', () => {
  test('create a task', async ({ adminPage }) => { /* ... */ });
  test('verify task appears in list', async ({ adminPage }) => { /* ... */ });
  test('delete the task', async ({ adminPage }) => { /* ... */ });
});
```

---

## How to Write a New Test

### 1. Create the spec file

```ts
// e2e/my-feature.spec.ts
import { test, expect, TEST_IDS } from './fixtures';

test.describe('My Feature', () => {
  test('renders correctly for admin', async ({ adminPage }) => {
    await adminPage.goto('/my-feature');
    await expect(adminPage.getByRole('heading', { name: 'My Feature' })).toBeVisible();
  });

  test('is read-only for viewers', async ({ viewerPage }) => {
    await viewerPage.goto('/my-feature');
    await expect(viewerPage.getByRole('button', { name: 'Edit' })).toBeDisabled();
  });
});
```

### 2. Add seed data (if needed)

If your feature requires data that doesn't exist yet:

1. Add `INSERT` statements to `e2e/seed.sql`
2. Add corresponding IDs to `TEST_IDS` in `e2e/fixtures.ts`
3. Add the table to the `DROP TABLE` list in `e2e/global-teardown.ts`

### 3. Run and verify

```bash
npx playwright test e2e/my-feature.spec.ts
```

---

## Seed Data

`seed.sql` creates the full schema and populates it with deterministic test data.

### What's seeded

| Entity              | Count | Examples                                     |
| ------------------- | ----- | -------------------------------------------- |
| Users               | 3     | admin, member, viewer                        |
| Tasks               | 10    | Various states: pending, running, completed, failed, awaiting |
| Task logs           | 5     | Execution logs for completed task            |
| Task messages       | 5     | Chat messages for completed and awaiting tasks |
| Task actions        | 10    | Agent actions including policy violations    |
| State transitions   | 3     | pending -> running -> completed              |
| Repo policies       | 2     | Tool and cost limits per repo                |
| Org policies        | 1     | Organization-wide defaults                   |
| Secrets             | 3     | Encrypted credential entries                 |
| Budgets             | 2     | User and org budget limits                   |
| Audit log entries   | 27+   | Auth, task, and admin events                 |
| Intake sources      | 2     | GitHub and Linear integrations               |
| Intake issues       | 8     | Various statuses: backlog, pending, done, failed |
| Intake poll log     | 6     | Polling history with durations               |

### TEST_IDS mapping

```ts
TEST_IDS.tasks.pending      // → "task-pending-1"     (seed.sql row)
TEST_IDS.tasks.completed    // → "task-completed-1"
TEST_IDS.users.admin        // → "testadmin"
TEST_IDS.repos.main         // → "testorg/testrepo"
TEST_IDS.intake.issues.backlogAuth  // → "Fix null pointer in auth module"
// ... see fixtures.ts for the full map
```

### Adding new seed data

1. Add your `INSERT` to `seed.sql` — use a stable, descriptive ID (e.g., `my-feature-entity-1`)
2. Add the ID to `TEST_IDS` in `fixtures.ts`
3. Add `DROP TABLE IF EXISTS your_table CASCADE;` to `global-teardown.ts` (if it's a new table)

---

## CI Pipeline

The GitHub Actions workflow (`.github/workflows/ci.yml`) has three jobs:

```
┌────────┐     ┌────────────┐     ┌──────────────┐
│  Rust  │     │  Frontend  │     │  E2E Tests   │
│ format │     │   lint     │     │ (depends on  │
│ clippy │────▶│   build    │────▶│  both jobs)  │
│  test  │     │            │     │              │
└────────┘     └────────────┘     └──────────────┘
```

### E2E job details

| Setting          | Value                        |
| ---------------- | ---------------------------- |
| Runner           | `ubuntu-latest`              |
| Timeout          | 30 minutes                   |
| Workers          | 4                            |
| Retries          | 2                            |
| Browser          | Chromium only                |
| PostgreSQL       | 16 (service container)       |
| DB name          | `tekton_test`                |
| DB credentials   | `tekton` / `tekton_test_password` |

### E2E job steps

1. Build backend (`cargo build --release`)
2. Install frontend deps (`npm ci`)
3. Build instrumented frontend (`INSTRUMENT_COVERAGE=true npm run build`)
4. Install Playwright (`npx playwright install --with-deps chromium`)
5. Start backend (serves `frontend/dist` as static files)
6. Health check (`curl http://localhost:3200/api/config`)
7. Run tests (`npx playwright test --project=chromium`)
8. Generate coverage report (`npx nyc report --reporter=text-summary`)
9. Enforce coverage thresholds (`npx nyc check-coverage`)

### Artifacts

Both are uploaded with **30-day retention**:

- `playwright-report/` — HTML test report with traces and screenshots
- `coverage/` — Istanbul code coverage report

---

## Code Coverage

### How it works

1. `vite-plugin-istanbul` instruments the production build when `INSTRUMENT_COVERAGE=true`
2. Each test fixture collects `window.__coverage__` from the browser after test completion
3. Coverage JSON files are saved to `.nyc_output/`
4. NYC merges all coverage data and generates reports

### Running locally

```bash
# Run everything in one command (build, test, report, threshold check)
make e2e.coverage

# Open the HTML report to see uncovered lines per file
open dashboard/frontend/coverage/index.html
```

Or step by step:

```bash
cd dashboard/frontend

# 1. Build with instrumentation
npm run build:coverage

# 2. Run E2E tests (collects coverage automatically)
npm run test:e2e

# 3. Generate reports
npm run coverage:report

# 4. Check thresholds
npm run coverage:check
```

### Thresholds

All thresholds are **70%**, configured in `.nycrc.json`:

| Metric     | Threshold |
| ---------- | --------- |
| Branches   | 70%       |
| Lines      | 70%       |
| Functions  | 70%       |
| Statements | 70%       |

CI enforces these via `npx nyc check-coverage` — the job fails if any metric drops below 70%.

### Exclusions

The following are excluded from coverage (`.nycrc.json`):

- `src/components/ui/**` — shadcn/ui primitives
- `src/components/VoiceInput.tsx` — browser speech API dependent
- `src/components/DiffViewer.tsx` — complex rendering component
- `src/components/LogViewer.tsx` — complex rendering component
- `src/components/TaskChat.tsx` — streaming/WebSocket dependent
- `src/components/BranchCombobox.tsx` — complex combobox
- `src/hooks/use-mobile.ts` — media query hook

---

## Lighthouse / Performance Tests

A separate Playwright project runs Lighthouse audits against key pages.

```bash
npm run test:lighthouse
```

### Thresholds

| Category       | Minimum Score |
| -------------- | ------------- |
| Performance    | 70            |
| Accessibility  | 85            |
| Best Practices | 85            |
| SEO            | 70            |

### Pages audited

- Home page (`/`)
- Tasks list (`/tasks`)
- Task detail (`/tasks/{id}`)
- Admin panel (`/admin`)
- Cost dashboard (`/cost`)

### How it works

Tests run serialized (`test.describe.serial`) with a single shared Chrome instance launched via `chrome-launcher`. Each page is audited with the admin auth cookie injected as an HTTP header. The desktop config uses simulated throttling (40ms RTT, 10Mbps throughput, no CPU slowdown).

> **Note:** Lighthouse tests have a 60-second timeout per page (vs 30 seconds for regular tests).

---

## Common Pitfalls

### `getByText` matching multiple elements

`getByText` does **substring matching** and matches across all element types. This frequently causes tests to pass in headless mode (smaller viewport, fewer elements visible) but fail in headed mode (larger viewport, more elements rendered).

**Example:** A page has both a `<label>Event Type</label>` filter control and a `<th>Event Type</th>` table header. `page.getByText('Event Type')` matches both, and Playwright's strict mode throws.

**Fix:** Use a more specific locator:

```ts
// BAD — matches label AND column header
await expect(page.getByText('Event Type')).toBeVisible();

// GOOD — scoped by role
await expect(page.locator('label', { hasText: 'Event Type' })).toBeVisible();
await expect(page.getByRole('columnheader', { name: 'Event Type' })).toBeVisible();

// GOOD — scoped by role for dialog headings
await expect(dialog.getByRole('heading', { name: 'Test Poll' })).toBeVisible();

// GOOD — scoped to a CSS class for badge-like elements
const labels = dialog.locator('span.rounded-full');
await expect(labels.filter({ hasText: 'bug' })).toBeVisible();

// OK — { exact: true } + scoped to a parent
await expect(row.getByText('On', { exact: true })).toBeVisible();

// OK — text is long/unique enough that duplicates are unlikely
await expect(dialog.getByText('Auth module throws NPE when token is expired.')).toBeVisible();
```

**Rule of thumb:** If the text is short or generic (`'Created'`, `'On'`, `'Error'`), always scope it — via a parent locator, a role, or a CSS selector. Prefer `getByRole` when the element has an obvious ARIA role (`heading`, `button`, `columnheader`, `cell`).
