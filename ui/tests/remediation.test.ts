import { describe, it, before, after } from 'node:test';
import assert from 'node:assert';
import { setupBrowser, teardownBrowser, newPage, navigateWithMock, clickAndWait } from './setup.ts';
import { buildMockScript } from './mock-tauri.ts';

const consolidateGroup = {
  group_id: 'consolidate-1',
  from_pattern: 'example.com',
  filters: [
    { id: 'f1', from: 'example.com', query: null, subject: null, add_label_ids: ['L1'], remove_label_ids: [] },
    { id: 'f2', from: 'example.com', query: null, subject: 'promo', add_label_ids: ['L1'], remove_label_ids: [] },
  ],
  label_names: ['Newsletters'],
  resolution_type: { Consolidate: { keep_filter_id: 'f1', remove_filter_ids: ['f2'] } },
};

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

  it('shows detect button in idle phase', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock());
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      const btn = await page.$('[data-testid="detect-btn"]');
      assert.ok(btn, 'Detect Overlaps button should be visible');
      const text = await page.$eval('[data-testid="detect-btn"]', el => el.textContent);
      assert.ok(text?.includes('Detect Overlaps'), 'Button should say Detect Overlaps');
    } finally {
      await page.close();
    }
  });

  it('transitions to decide phase after detecting groups', async () => {
    const mockGroups = [
      {
        group_id: 'group-1',
        from_pattern: 'example.com',
        filters: [
          { id: 'f1', from: 'example.com', query: null, subject: null, add_label_ids: ['L1'], remove_label_ids: [] },
          { id: 'f2', from: 'example.com', query: null, subject: 'promo', add_label_ids: ['L1'], remove_label_ids: [] },
        ],
        label_names: ['Newsletters'],
        resolution_type: { Consolidate: { keep_filter_id: 'f1', remove_filter_ids: ['f2'] } },
      },
    ];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: mockGroups }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const heading = await page.$eval('[data-testid="decide-header"]', el => el.textContent);
      assert.ok(heading?.includes('1'), 'Should show 1 overlap group found');
    } finally {
      await page.close();
    }
  });

  it('shows empty state when no overlaps detected', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: [] }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const msg = await page.$eval('[data-testid="no-overlaps"]', el => el.textContent);
      assert.ok(msg?.includes('No overlapping filters'), 'Should show no overlaps message');
    } finally {
      await page.close();
    }
  });

  it('renders consolidate card with accept and skip buttons', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: [consolidateGroup] }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const card = await page.$('[data-testid="group-card-consolidate-1"]');
      assert.ok(card, 'Consolidate card should be rendered');
      const acceptBtn = await page.$('[data-testid="accept-consolidate-1"]');
      assert.ok(acceptBtn, 'Accept button should exist');
      const skipBtn = await page.$('[data-testid="skip-consolidate-1"]');
      assert.ok(skipBtn, 'Skip button should exist');
    } finally {
      await page.close();
    }
  });

  it('marks consolidate card as decided after accepting', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: [consolidateGroup] }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      const borderClass = await page.$eval('[data-testid="group-card-consolidate-1"]', el => el.className);
      assert.ok(borderClass.includes('border-l-green'), 'Card should have green left border when decided');
    } finally {
      await page.close();
    }
  });
});
