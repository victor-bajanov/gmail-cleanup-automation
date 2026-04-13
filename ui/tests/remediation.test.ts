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

const mechanicalFixGroup = {
  group_id: 'mechfix-1',
  from_pattern: 'store.com',
  filters: [
    { id: 'f3', from: 'store.com', query: null, subject: null, add_label_ids: ['L2'], remove_label_ids: [] },
    { id: 'f4', from: 'store.com', query: null, subject: 'receipt', add_label_ids: ['L3'], remove_label_ids: [] },
  ],
  label_names: ['Shopping', 'Receipts'],
  resolution_type: {
    MechanicalFix: {
      proposed_replacements: [
        {
          id: null, name: 'store.com', from_pattern: 'store.com',
          is_specific_sender: false, excluded_senders: [],
          subject_keywords: [], excluded_subject_patterns: ['receipt'],
          target_label_id: 'L2', should_archive: false,
        },
        {
          id: null, name: 'store.com-receipt', from_pattern: 'store.com',
          is_specific_sender: false, excluded_senders: [],
          subject_keywords: ['receipt'], excluded_subject_patterns: [],
          target_label_id: 'L3', should_archive: false,
        },
      ],
    },
  },
};

const pickWinnerGroup = {
  group_id: 'pick-1',
  from_pattern: 'news.com',
  filters: [
    { id: 'f5', from: 'news.com', query: null, subject: null, add_label_ids: ['L4'], remove_label_ids: [] },
    { id: 'f6', from: 'news.com', query: null, subject: 'digest', add_label_ids: ['L5'], remove_label_ids: [] },
  ],
  label_names: ['News', 'Digests'],
  resolution_type: 'PickWinner' as const,
};

function baseMock(overrides: Record<string, unknown> = {}): string {
  return buildMockScript({
    'plugin:event|listen': 0,
    check_auth_status: { authenticated: true, email: 'test@example.com', credentials_path: '', credentials_exist: true, token_exists: true },
    initialize_client: true,
    detect_overlaps: { groups: [], label_id_to_name: {} },
    get_config_settings: { label_prefix: '', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
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
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: mockGroups, label_id_to_name: {} } }));
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
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [], label_id_to_name: {} } }));
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
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} } }));
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
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} } }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      const borderClass = await page.$eval('[data-testid="group-card-consolidate-1"]', el => el.className);
      assert.ok(borderClass.includes('border-l-green'), 'Card should have green left border when decided');
    } finally {
      await page.close();
    }
  });

  it('renders mechanical fix card with accept fix and skip buttons', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [mechanicalFixGroup], label_id_to_name: {} } }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const card = await page.$('[data-testid="group-card-mechfix-1"]');
      assert.ok(card, 'MechanicalFix card should be rendered');
      const acceptBtn = await page.$('[data-testid="accept-mechfix-1"]');
      assert.ok(acceptBtn, 'Accept Fix button should exist');
      const text = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.textContent);
      assert.ok(text?.includes('Auto-fixable'), 'Should indicate auto-fixable');
    } finally {
      await page.close();
    }
  });

  it('renders pick winner card with clickable filter rows', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [pickWinnerGroup], label_id_to_name: {} } }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const row1 = await page.$('[data-testid="filter-row-f5"]');
      const row2 = await page.$('[data-testid="filter-row-f6"]');
      assert.ok(row1, 'Filter row f5 should exist');
      assert.ok(row2, 'Filter row f6 should exist');
    } finally {
      await page.close();
    }
  });

  it('highlights selected winner and marks card as decided', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups: [pickWinnerGroup], label_id_to_name: {} } }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="filter-row-f5"]');
      const classes = await page.$eval('[data-testid="filter-row-f5"]', el => el.className);
      assert.ok(classes.includes('ring-2') || classes.includes('border-primary') || classes.includes('bg-primary'), 'Selected row should be visually highlighted');
      const cardClasses = await page.$eval('[data-testid="group-card-pick-1"]', el => el.className);
      assert.ok(cardClasses.includes('border-l-green'), 'Card should show decided state');
    } finally {
      await page.close();
    }
  });

  it('enables Review Plan button only when all groups are decided', async () => {
    const groups = [consolidateGroup, pickWinnerGroup];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({ detect_overlaps: { groups, label_id_to_name: {} } }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');

      // Button should be disabled initially
      const disabledAttr = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
      assert.strictEqual(disabledAttr, true, 'Review Plan should be disabled with undecided groups');

      // Decide on first group
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');

      // Still disabled (one group undecided)
      const stillDisabled = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
      assert.strictEqual(stillDisabled, true, 'Should still be disabled with 1 undecided');

      // Decide on second group
      await clickAndWait(page, '[data-testid="filter-row-f5"]');

      // Now enabled
      const nowEnabled = await page.$eval('[data-testid="review-plan-btn"]', el => (el as HTMLButtonElement).disabled);
      assert.strictEqual(nowEnabled, false, 'Should be enabled when all decided');
    } finally {
      await page.close();
    }
  });

  it('shows summary and execute button in confirm phase', async () => {
    const summaryText = 'Remediation plan: 1 groups, 1 deletions, 0 creations, 0 skipped\n  consolidate-1 — consolidate (same label), delete 1 redundant filters';
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} },
        remediation_summary: summaryText,
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      await clickAndWait(page, '[data-testid="review-plan-btn"]');

      const summary = await page.$eval('[data-testid="plan-summary"]', el => el.textContent);
      assert.ok(summary?.includes('1 deletions'), 'Should show summary text');

      const executeBtn = await page.$('[data-testid="execute-btn"]');
      assert.ok(executeBtn, 'Execute button should exist');

      const backBtn = await page.$('[data-testid="back-to-decisions-btn"]');
      assert.ok(backBtn, 'Back to Decisions button should exist');
    } finally {
      await page.close();
    }
  });

  it('shows execution results with apply button when swaps exist', async () => {
    const swaps = [
      { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
    ];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} },
        remediation_summary: 'plan',
        execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
        collect_remediation_swaps: swaps,
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      await clickAndWait(page, '[data-testid="review-plan-btn"]');
      await clickAndWait(page, '[data-testid="execute-btn"]');

      const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
      assert.ok(deleted?.includes('1'), 'Should show 1 deleted');

      const applyBtn = await page.$('[data-testid="apply-swaps-btn"]');
      assert.ok(applyBtn, 'Apply Label Changes button should exist');
    } finally {
      await page.close();
    }
  });

  it('shows relabeled count after applying swaps', async () => {
    const swaps = [
      { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
    ];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} },
        remediation_summary: 'plan',
        execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
        collect_remediation_swaps: swaps,
        apply_remediation_swaps: { messages_relabeled: 42, messages_failed: 0, errors: [] },
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      await clickAndWait(page, '[data-testid="review-plan-btn"]');
      await clickAndWait(page, '[data-testid="execute-btn"]');
      await clickAndWait(page, '[data-testid="apply-swaps-btn"]');

      const relabeled = await page.$eval('[data-testid="apply-relabeled"]', el => el.textContent);
      assert.ok(relabeled?.includes('42'), 'Should show 42 messages relabeled');
    } finally {
      await page.close();
    }
  });

  it('completes full flow: detect -> decide -> confirm -> execute -> apply -> done', async () => {
    const groups = [consolidateGroup, pickWinnerGroup];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups, label_id_to_name: {} },
        remediation_summary: 'Remediation plan: 2 groups, 2 deletions, 0 creations, 0 skipped',
        execute_remediation: { deleted: ['f2', 'f6'], created: [], skipped: 0, errors: [] },
        collect_remediation_swaps: [
          { query: 'from:news.com', add_label_id: 'L4', remove_label_ids: ['L5'] },
        ],
        apply_remediation_swaps: { messages_relabeled: 10, messages_failed: 0, errors: [] },
      }));

      // Navigate
      await clickAndWait(page, '[data-testid="nav-remediation"]');

      // Detect
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const header = await page.$eval('[data-testid="decide-header"]', el => el.textContent);
      assert.ok(header?.includes('2'), 'Should show 2 groups');

      // Decide
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      await clickAndWait(page, '[data-testid="filter-row-f5"]');

      // Review
      await clickAndWait(page, '[data-testid="review-plan-btn"]');
      const summary = await page.$('[data-testid="plan-summary"]');
      assert.ok(summary, 'Should show plan summary');

      // Execute
      await clickAndWait(page, '[data-testid="execute-btn"]');
      const deleted = await page.$eval('[data-testid="result-deleted"]', el => el.textContent);
      assert.ok(deleted?.includes('2'), 'Should show 2 deleted');

      // Apply
      await clickAndWait(page, '[data-testid="apply-swaps-btn"]');
      const relabeled = await page.$eval('[data-testid="apply-relabeled"]', el => el.textContent);
      assert.ok(relabeled?.includes('10'), 'Should show 10 relabeled');

      // Done
      await clickAndWait(page, '[data-testid="done-btn"]');
      const detectBtn = await page.$('[data-testid="detect-btn"]');
      assert.ok(detectBtn, 'Should be back at idle phase with detect button');
    } finally {
      await page.close();
    }
  });

  it('displays label names instead of IDs in pick-winner rows', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: {
          groups: [pickWinnerGroup],
          label_id_to_name: { L4: 'automanaged/news/news-com', L5: 'automanaged/digests/news-com' },
        },
        get_config_settings: { label_prefix: 'automanaged', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      const row1Text = await page.$eval('[data-testid="filter-row-f5"]', el => el.textContent);
      assert.ok(row1Text?.includes('news/news-com'), `Expected label name but got: ${row1Text}`);
      assert.ok(!row1Text?.includes('L4'), `Should not show raw ID L4: ${row1Text}`);
    } finally {
      await page.close();
    }
  });

  it('highlights differing label segments in group header', async () => {
    const group = {
      group_id: 'diff-1',
      from_pattern: 'airbnb.com',
      filters: [
        { id: 'f10', from: 'airbnb.com', query: null, subject: null, add_label_ids: ['L10'], remove_label_ids: [] },
        { id: 'f11', from: 'airbnb.com', query: null, subject: 'receipt', add_label_ids: ['L11'], remove_label_ids: [] },
      ],
      label_names: ['automanaged/other/airbnb-com', 'automanaged/marketing/airbnb-com'],
      resolution_type: 'PickWinner' as const,
    };
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [group], label_id_to_name: { L10: 'automanaged/other/airbnb-com', L11: 'automanaged/marketing/airbnb-com' } },
        get_config_settings: { label_prefix: 'automanaged', scan_query: '', max_results: 500, excluded_labels: [], auto_archive: false },
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');

      const highlighted = await page.$$eval('[data-testid="group-card-diff-1"] [data-highlighted="true"]', els => els.map(el => el.textContent));
      assert.ok(highlighted.includes('other'), `Should highlight "other", got: ${JSON.stringify(highlighted)}`);
      assert.ok(highlighted.includes('marketing'), `Should highlight "marketing", got: ${JSON.stringify(highlighted)}`);
    } finally {
      await page.close();
    }
  });

  it('allows picking a winner on a mechanical fix card by clicking a filter label', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [mechanicalFixGroup], label_id_to_name: { L2: 'Shopping', L3: 'Receipts' } },
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="pick-filter-f3"]');
      const borderClass = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.className);
      assert.ok(borderClass.includes('border-l-green'), 'Card should show decided state after picking winner');
    } finally {
      await page.close();
    }
  });

  it('accepts mechanical fix with A key and picks winner with number keys', async () => {
    const groups = [mechanicalFixGroup, pickWinnerGroup];
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups, label_id_to_name: { L2: 'Shopping', L3: 'Receipts', L4: 'News', L5: 'Digests' } },
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');

      // Press A to accept first (mechfix) group
      await page.keyboard.press('a');
      await new Promise(r => setTimeout(r, 300));
      const mechfixClass = await page.$eval('[data-testid="group-card-mechfix-1"]', el => el.className);
      assert.ok(mechfixClass.includes('border-l-green'), 'MechFix card should be decided after A key');

      // Press 1 to pick first filter as winner on second (pick-winner) group
      await page.keyboard.press('1');
      await new Promise(r => setTimeout(r, 300));
      const pickClass = await page.$eval('[data-testid="group-card-pick-1"]', el => el.className);
      assert.ok(pickClass.includes('border-l-green'), 'PickWinner card should be decided after 1 key');
    } finally {
      await page.close();
    }
  });

  it('shows no label changes needed when no swaps', async () => {
    const page = await newPage();
    try {
      await navigateWithMock(page, baseMock({
        detect_overlaps: { groups: [consolidateGroup], label_id_to_name: {} },
        remediation_summary: 'plan',
        execute_remediation: { deleted: ['f2'], created: [], skipped: 0, errors: [] },
        collect_remediation_swaps: [],
      }));
      await clickAndWait(page, '[data-testid="nav-remediation"]');
      await clickAndWait(page, '[data-testid="detect-btn"]');
      await clickAndWait(page, '[data-testid="accept-consolidate-1"]');
      await clickAndWait(page, '[data-testid="review-plan-btn"]');
      await clickAndWait(page, '[data-testid="execute-btn"]');

      const msg = await page.$eval('[data-testid="no-swaps"]', el => el.textContent);
      assert.ok(msg?.includes('No label changes'), 'Should show no swaps message');
    } finally {
      await page.close();
    }
  });
});
