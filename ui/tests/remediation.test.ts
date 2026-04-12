import { describe, it, before, after } from 'node:test';
import assert from 'node:assert';
import { setupBrowser, teardownBrowser, newPage, navigateWithMock, clickAndWait } from './setup.ts';
import { buildMockScript } from './mock-tauri.ts';

function baseMock(overrides: Record<string, unknown> = {}): string {
  return buildMockScript({
    'plugin:event|listen': 0,
    check_auth_status: { authenticated: true, email: 'test@example.com', credentials_path: '', credentials_exist: true, token_exists: true },
    initialize_client: true,
    detect_overlaps: [],
    submit_group_decision: null,
    execute_remediation: { deleted: [], created: [], skipped: 0, errors: [] },
    remediation_summary: 'Remediation plan: 0 groups, 0 deletions, 0 creations, 0 skipped',
    collect_remediation_swaps: [],
    apply_remediation_swaps: { messages_relabeled: 0, messages_failed: 0, errors: [] },
    ...overrides,
  });
}

describe('Remediation UI', () => {
  before(async () => { await setupBrowser(); });
  after(async () => { await teardownBrowser(); });

  it('should render without crashing', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock());
      const body = await page.$('body');
      assert.ok(body, 'Page body should exist');
    } finally {
      await page.close();
    }
  });

  it('navigates to remediation view via sidebar', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock());
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      const heading = await page.$eval('[data-testid="remediation-view"]', el => el.textContent);
      assert.ok(heading, 'Remediation view should be rendered');
    } finally {
      await page.close();
    }
  });
});
