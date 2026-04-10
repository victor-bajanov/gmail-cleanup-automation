# Tauri UI Bugfixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 6 bugs/improvements in the Tauri GUI: OR clause parsing, label ID resolution, archive toggle shortcut, custom label UX, collapsible overlap section, and apply filters progress.

**Architecture:** Rust-side fixes in `filter_ast.rs` and `filter_overlap.rs` for the OR parsing and label resolution. Frontend-only changes in SolidJS for the remaining 4 fixes. All changes are independent and can be tested in isolation.

**Tech Stack:** Rust (filter engine), SolidJS + TypeScript (frontend), Tauri IPC

---

### Task 1: Add `MultipleSenders` variant to `FromClause`

**Files:**
- Modify: `src/filter_ast.rs:150-175`

**Step 1: Add the variant**

In `src/filter_ast.rs`, add `MultipleSenders` to the `FromClause` enum (after line 156):

```rust
pub enum FromClause {
    /// Matches all emails from a domain (e.g., *@github.com)
    Domain(DomainPattern),
    /// Matches emails from a specific sender (e.g., noreply@github.com)
    SpecificSender(EmailPattern),
    /// Matches emails from any of multiple senders (OR clause)
    MultipleSenders(Vec<FromClause>),
}
```

**Step 2: Update `describe()` and `domain()` on `FromClause`**

Update the `impl FromClause` block:

```rust
impl FromClause {
    pub fn describe(&self) -> String {
        match self {
            FromClause::Domain(d) => d.describe(),
            FromClause::SpecificSender(e) => e.full_address(),
            FromClause::MultipleSenders(senders) => {
                senders.iter().map(|s| s.describe()).collect::<Vec<_>>().join(" OR ")
            }
        }
    }

    pub fn domain(&self) -> &str {
        match self {
            FromClause::Domain(d) => &d.domain,
            FromClause::SpecificSender(e) => &e.domain,
            FromClause::MultipleSenders(senders) => {
                // Return domain of first sender as representative
                senders.first().map(|s| s.domain()).unwrap_or("")
            }
        }
    }
}
```

**Step 3: Fix all match arms that don't handle `MultipleSenders`**

In `src/filter_ast.rs`, update `ExclusionClause` match arms in `to_gmail_query` and any other exhaustive matches. The compiler will guide you — fix every "non-exhaustive patterns" error.

**Step 4: Run `cargo check` to find remaining compile errors**

Run: `cd /Users/victorb/code/gmail-cleanup && cargo check 2>&1`
Expected: Compile errors in `filter_overlap.rs` for unhandled `MultipleSenders` — those are Task 2.

**Step 5: Commit**

```bash
git add src/filter_ast.rs
git commit -m "feat: add MultipleSenders variant to FromClause for OR queries"
```

---

### Task 2: Update `parse_gmail_query` to handle OR clauses

**Files:**
- Modify: `src/filter_overlap.rs:825-886` (parser)
- Modify: `src/filter_overlap.rs:888-924` (to_gmail_query)

**Step 1: Write failing test for OR query parsing**

Add to `src/filter_overlap.rs` tests module:

```rust
#[test]
fn test_parse_gmail_query_or_senders() {
    let expr = parse_gmail_query(
        "from:(allsales@powerbuys.com.au OR dailydeals@powerbuys.com.au)"
    );
    assert!(expr.from_clause.is_some());
    if let Some(FromClause::MultipleSenders(senders)) = &expr.from_clause {
        assert_eq!(senders.len(), 2);
        assert!(matches!(&senders[0], FromClause::SpecificSender(e) if e.full_address() == "allsales@powerbuys.com.au"));
        assert!(matches!(&senders[1], FromClause::SpecificSender(e) if e.full_address() == "dailydeals@powerbuys.com.au"));
    } else {
        panic!("Expected MultipleSenders, got {:?}", expr.from_clause);
    }
}

#[test]
fn test_parse_gmail_query_or_mixed_domain_and_sender() {
    let expr = parse_gmail_query("from:(*@github.com OR noreply@example.com)");
    assert!(expr.from_clause.is_some());
    if let Some(FromClause::MultipleSenders(senders)) = &expr.from_clause {
        assert_eq!(senders.len(), 2);
        assert!(matches!(&senders[0], FromClause::Domain(_)));
        assert!(matches!(&senders[1], FromClause::SpecificSender(_)));
    } else {
        panic!("Expected MultipleSenders, got {:?}", expr.from_clause);
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test test_parse_gmail_query_or -- --nocapture 2>&1`
Expected: FAIL

**Step 3: Update `parse_gmail_query`**

Replace the from-parsing block (lines 828-844) with:

```rust
// Parse from: clause
if let Some(from_start) = query.find("from:(") {
    let from_end = query[from_start + 6..].find(')').map(|i| from_start + 6 + i);
    if let Some(end) = from_end {
        let from_pattern = &query[from_start + 6..end];

        // Check for OR clause (multiple senders)
        if from_pattern.contains(" OR ") {
            let parts: Vec<&str> = from_pattern.split(" OR ").collect();
            let mut senders: Vec<FromClause> = Vec::new();
            for part in parts {
                let part = part.trim();
                if part.starts_with("*@") {
                    let domain = part.trim_start_matches("*@");
                    senders.push(FromClause::Domain(DomainPattern::new(domain)));
                } else if let Some(email) = EmailPattern::parse(part) {
                    senders.push(FromClause::SpecificSender(email));
                }
            }
            if senders.len() == 1 {
                expr.from_clause = Some(senders.into_iter().next().unwrap());
            } else if senders.len() > 1 {
                expr.from_clause = Some(FromClause::MultipleSenders(senders));
            }
        } else if from_pattern.starts_with("*@") {
            let domain = from_pattern.trim_start_matches("*@");
            expr.from_clause = Some(FromClause::Domain(DomainPattern::new(domain)));
        } else if from_pattern.contains('@') {
            if let Some(email) = EmailPattern::parse(from_pattern) {
                expr.from_clause = Some(FromClause::SpecificSender(email));
            }
        }
    }
}
```

**Step 4: Update `to_gmail_query`**

Add the `MultipleSenders` match arm in the `to_gmail_query` function (around line 892):

```rust
if let Some(ref from) = expr.from_clause {
    match from {
        FromClause::Domain(d) => {
            if d.include_subdomains {
                parts.push(format!("from:(*@*.{})", d.domain));
            } else {
                parts.push(format!("from:(*@{})", d.domain));
            }
        }
        FromClause::SpecificSender(e) => {
            parts.push(format!("from:({})", e.full_address()));
        }
        FromClause::MultipleSenders(senders) => {
            let sender_strs: Vec<String> = senders.iter().map(|s| match s {
                FromClause::Domain(d) => {
                    if d.include_subdomains {
                        format!("*@*.{}", d.domain)
                    } else {
                        format!("*@{}", d.domain)
                    }
                }
                FromClause::SpecificSender(e) => e.full_address(),
                FromClause::MultipleSenders(_) => s.describe(), // nested, shouldn't happen
            }).collect();
            parts.push(format!("from:({})", sender_strs.join(" OR ")));
        }
    }
}
```

Also update the exclusion match arms to handle `MultipleSenders`:

```rust
for exclusion in &expr.exclusions {
    match &exclusion.pattern {
        FromClause::Domain(d) => {
            parts.push(format!("-from:(*@{})", d.domain));
        }
        FromClause::SpecificSender(e) => {
            parts.push(format!("-from:({})", e.full_address()));
        }
        FromClause::MultipleSenders(senders) => {
            for s in senders {
                match s {
                    FromClause::Domain(d) => parts.push(format!("-from:(*@{})", d.domain)),
                    FromClause::SpecificSender(e) => parts.push(format!("-from:({})", e.full_address())),
                    _ => {}
                }
            }
        }
    }
}
```

**Step 5: Run tests**

Run: `cargo test test_parse_gmail_query_or -- --nocapture 2>&1`
Expected: PASS

**Step 6: Commit**

```bash
git add src/filter_overlap.rs
git commit -m "feat: parse OR clauses in from: patterns as MultipleSenders"
```

---

### Task 3: Update overlap analyzer for `MultipleSenders`

**Files:**
- Modify: `src/filter_overlap.rs:355-421` (analyze_from_relation)
- Modify: `src/filter_overlap.rs:480-509` (exclusions_resolve_overlap)

**Step 1: Write failing test for OR vs single sender analysis**

```rust
#[test]
fn test_multiple_senders_vs_single_sender_disjoint() {
    let analyzer = FilterOverlapAnalyzer::new();
    // from:(a@x.com OR b@x.com) vs from:(c@y.com) -> Disjoint
    let multi = FromClause::MultipleSenders(vec![
        FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
        FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
    ]);
    let single = FromClause::SpecificSender(EmailPattern::new("c", "y.com"));
    let relation = analyzer.analyze_from_relation(&multi, &single);
    assert_eq!(relation, PatternRelation::Disjoint);
}

#[test]
fn test_multiple_senders_vs_single_sender_subsumes() {
    let analyzer = FilterOverlapAnalyzer::new();
    // from:(a@x.com OR b@x.com) vs from:(a@x.com) -> Subsumes (multi contains single)
    let multi = FromClause::MultipleSenders(vec![
        FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
        FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
    ]);
    let single = FromClause::SpecificSender(EmailPattern::new("a", "x.com"));
    let relation = analyzer.analyze_from_relation(&multi, &single);
    assert_eq!(relation, PatternRelation::Subsumes);
}

#[test]
fn test_single_sender_vs_multiple_senders_subsumed() {
    let analyzer = FilterOverlapAnalyzer::new();
    // from:(a@x.com) vs from:(a@x.com OR b@x.com) -> SubsumedBy
    let single = FromClause::SpecificSender(EmailPattern::new("a", "x.com"));
    let multi = FromClause::MultipleSenders(vec![
        FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
        FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
    ]);
    let relation = analyzer.analyze_from_relation(&single, &multi);
    assert_eq!(relation, PatternRelation::SubsumedBy);
}

#[test]
fn test_domain_subsumes_multiple_senders_same_domain() {
    let analyzer = FilterOverlapAnalyzer::new();
    // from:(*@x.com) vs from:(a@x.com OR b@x.com) -> Subsumes
    let domain = FromClause::Domain(DomainPattern::new("x.com"));
    let multi = FromClause::MultipleSenders(vec![
        FromClause::SpecificSender(EmailPattern::new("a", "x.com")),
        FromClause::SpecificSender(EmailPattern::new("b", "x.com")),
    ]);
    let relation = analyzer.analyze_from_relation(&domain, &multi);
    assert_eq!(relation, PatternRelation::Subsumes);
}

#[test]
fn test_powerbuys_vs_github_disjoint() {
    // The exact bug report scenario
    let analyzer = FilterOverlapAnalyzer::new();
    let expr_a = parse_gmail_query(
        "from:(allsales@powerbuys.com.au OR dailydeals@powerbuys.com.au)"
    );
    let expr_b = parse_gmail_query("from:(noreply@github.com)");
    let relation = analyzer.analyze_expr_relation(&expr_a, &expr_b);
    assert_eq!(relation, PatternRelation::Disjoint);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test test_multiple_senders -- --nocapture 2>&1 && cargo test test_powerbuys -- --nocapture 2>&1`
Expected: FAIL (compile error — `MultipleSenders` not handled in `analyze_from_relation`)

**Step 3: Update `analyze_from_relation`**

Replace the match in `analyze_from_relation` (lines 356-392):

```rust
pub fn analyze_from_relation(&self, from_a: &FromClause, from_b: &FromClause) -> PatternRelation {
    match (from_a, from_b) {
        // Domain vs Domain
        (FromClause::Domain(da), FromClause::Domain(db)) => {
            self.analyze_domain_relation(da, db)
        }

        // Domain vs SpecificSender
        (FromClause::SpecificSender(email), FromClause::Domain(domain)) => {
            if domain.matches_domain(&email.domain) {
                PatternRelation::SubsumedBy
            } else {
                PatternRelation::Disjoint
            }
        }

        (FromClause::Domain(domain), FromClause::SpecificSender(email)) => {
            if domain.matches_domain(&email.domain) {
                PatternRelation::Subsumes
            } else {
                PatternRelation::Disjoint
            }
        }

        // SpecificSender vs SpecificSender
        (FromClause::SpecificSender(a), FromClause::SpecificSender(b)) => {
            if a.matches(b) {
                PatternRelation::Identical
            } else {
                PatternRelation::Disjoint
            }
        }

        // MultipleSenders vs single pattern:
        // A(multi) subsumes B if any sender in A subsumes/is-identical-to B
        // A(multi) is subsumed by B if B subsumes every sender in A
        (FromClause::MultipleSenders(senders), other) => {
            self.analyze_multi_vs_single(senders, other)
        }

        // Single vs MultipleSenders: reverse
        (other, FromClause::MultipleSenders(senders)) => {
            let reversed = self.analyze_multi_vs_single(senders, other);
            match reversed {
                PatternRelation::Subsumes => PatternRelation::SubsumedBy,
                PatternRelation::SubsumedBy => PatternRelation::Subsumes,
                other => other,
            }
        }
    }
}
```

**Step 4: Add `analyze_multi_vs_single` helper**

Add this method to `FilterOverlapAnalyzer`:

```rust
/// Analyzes MultipleSenders(senders) vs a single FromClause pattern.
/// Returns the relation from the MultipleSenders' perspective.
fn analyze_multi_vs_single(&self, senders: &[FromClause], other: &FromClause) -> PatternRelation {
    let mut any_subsumes = false;    // Does any sender in multi subsume other?
    let mut any_identical = false;   // Is any sender identical to other?
    let mut any_overlap = false;     // Does any sender overlap with other?
    let mut all_subsumed = true;     // Is every sender subsumed by other?

    for sender in senders {
        let relation = self.analyze_from_relation(sender, other);
        match relation {
            PatternRelation::Identical => {
                any_identical = true;
                // identical means also subsumed
            }
            PatternRelation::Subsumes => {
                any_subsumes = true;
            }
            PatternRelation::SubsumedBy => {
                // this sender is subsumed by other, good for all_subsumed check
            }
            PatternRelation::Overlaps { .. } => {
                any_overlap = true;
                all_subsumed = false;
            }
            PatternRelation::Disjoint => {
                all_subsumed = false;
            }
        }
    }

    if all_subsumed {
        // Every sender in multi is subsumed by other -> multi is subsumed
        PatternRelation::SubsumedBy
    } else if any_subsumes || any_identical {
        // Multi contains other (at least one sender covers other)
        // But multi also has senders beyond other, so it's broader
        if senders.len() == 1 && any_identical {
            PatternRelation::Identical
        } else {
            PatternRelation::Subsumes
        }
    } else if any_overlap {
        PatternRelation::Overlaps {
            description: "Partial overlap with multi-sender filter".to_string(),
        }
    } else {
        PatternRelation::Disjoint
    }
}
```

**Step 5: Run tests**

Run: `cargo test test_multiple_senders -- --nocapture 2>&1 && cargo test test_powerbuys -- --nocapture 2>&1`
Expected: PASS

**Step 6: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass (fix any remaining compile errors from exhaustive match)

**Step 7: Commit**

```bash
git add src/filter_overlap.rs
git commit -m "feat: handle MultipleSenders in overlap analyzer"
```

---

### Task 4: Fix label ID vs name comparison in analysis

**Files:**
- Modify: `src-tauri/src/commands/analysis.rs:163-199`

**Step 1: Update existing filter construction to use resolved label name**

At line 167, change:

```rust
// BEFORE (line 167):
FilterActions::with_label(&label_id),

// AFTER:
FilterActions::with_label(&label_name),
```

The `label_name` variable is already computed at line 142.

**Step 2: Update proposed filter construction to use resolved label name**

At lines 195-199, change:

```rust
// BEFORE:
if f.should_archive {
    FilterActions::label_and_archive(&f.target_label_id)
} else {
    FilterActions::with_label(&f.target_label_id)
},

// AFTER:
if f.should_archive {
    FilterActions::label_and_archive(&label_name)
} else {
    FilterActions::with_label(&label_name)
},
```

The `label_name` for proposed filters is already computed at line 178.

**Step 3: Verify it compiles**

Run: `cargo check 2>&1`
Expected: Clean compile

**Step 4: Commit**

```bash
git add src-tauri/src/commands/analysis.rs
git commit -m "fix: use resolved label names in overlap analysis instead of raw IDs"
```

---

### Task 5: Implement "A" shortcut to toggle auto-archive

**Files:**
- Modify: `ui/src/components/views/ReviewView.tsx:85-87`

**Step 1: Implement the archive toggle**

Replace the `'a'` case (lines 85-87):

```typescript
case 'a': {
  // Toggle archive on current cluster
  const idx = selectedIndex();
  if (idx === null) break;
  const updated = review.clusters().map((c, i) =>
    i === idx ? { ...c, should_archive: !c.should_archive } : c
  );
  review.setClusters(updated);
  break;
}
```

**Step 2: Add "A" to the keyboard shortcuts legend**

Find the shortcuts list (around line 578) and add after the "L" entry:

```tsx
<div><kbd class="kbd">A</kbd> Toggle archive</div>
```

**Step 3: Add an archive toggle button to the action bar**

Add after the "Custom Label" button (after line 473):

```tsx
<button
  onClick={() => {
    const idx = selectedIndex();
    if (idx === null) return;
    const updated = review.clusters().map((c, i) =>
      i === idx ? { ...c, should_archive: !c.should_archive } : c
    );
    review.setClusters(updated);
  }}
  disabled={isSubmitting()}
  class="btn-ghost flex items-center gap-2"
>
  <kbd class="kbd">A</kbd>
  Archive: {currentCluster()?.should_archive ? 'ON' : 'OFF'}
</button>
```

**Step 4: Verify in browser (manual)**

Run: `cd /Users/victorb/code/gmail-cleanup && cargo tauri dev`
Test: Navigate to review, press A, verify archive toggles.

**Step 5: Commit**

```bash
git add ui/src/components/views/ReviewView.tsx
git commit -m "feat: add A shortcut to toggle auto-archive during review"
```

---

### Task 6: Prepopulate custom label with suggested label

**Files:**
- Modify: `ui/src/components/views/ReviewView.tsx:82-83`

**Step 1: Prepopulate on L press**

Change the `'l'` case (lines 82-83):

```typescript
case 'l':
  setCustomLabel(currentCluster()?.suggested_label || '');
  setShowCustomLabel(true);
  break;
```

**Step 2: Also prepopulate when clicking the "Custom Label" button**

Update the button onClick (line 467):

```tsx
onClick={() => {
  setCustomLabel(currentCluster()?.suggested_label || '');
  setShowCustomLabel(true);
}}
```

**Step 3: Commit**

```bash
git add ui/src/components/views/ReviewView.tsx
git commit -m "feat: prepopulate custom label input with suggested label"
```

---

### Task 7: Add category quick-change dropdown to custom label

**Files:**
- Modify: `ui/src/components/views/ReviewView.tsx:383-421`
- Modify: `ui/src/lib/api.ts` (add `getLabelCategories` wrapper)

**Step 1: Add categories signal and load on mount**

Add near the top of `ReviewView` (after the existing signals around line 10):

```typescript
const [categories, setCategories] = createSignal<string[]>([]);
```

Add to the existing `onMount` (after the summary load around line 46):

```typescript
// Load label categories for the quick-change dropdown
try {
  const editorData = await api.editorGetFilters(false);
  const labelNames = Object.values(editorData.label_map);
  // Extract unique middle segments from Prefix/Category/Rest pattern
  const cats = new Set<string>();
  for (const name of labelNames) {
    const parts = name.split('/');
    if (parts.length >= 2) {
      cats.add(parts[1]);
    }
  }
  setCategories([...cats].sort());
} catch (e) {
  console.warn('Failed to load label categories:', e);
}
```

**Step 2: Add category dropdown to the custom label UI**

Replace the custom label input section (lines 383-421) with:

```tsx
<Show when={showCustomLabel()}>
  <div class="p-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-700">
    {/* Category quick-change */}
    <Show when={categories().length > 0 && customLabel().includes('/')}>
      <div class="flex gap-1 mb-2 flex-wrap">
        <span class="text-xs text-gray-500 dark:text-gray-400 self-center mr-1">Category:</span>
        <For each={categories()}>
          {(cat) => {
            const parts = customLabel().split('/');
            const isActive = parts.length >= 2 && parts[1] === cat;
            return (
              <button
                class="text-xs px-2 py-1 rounded transition-colors"
                classList={{
                  'bg-primary-100 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300': isActive,
                  'bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-500': !isActive,
                }}
                onClick={() => {
                  const parts = customLabel().split('/');
                  if (parts.length >= 3) {
                    parts[1] = cat;
                    setCustomLabel(parts.join('/'));
                  } else if (parts.length === 2) {
                    parts[1] = cat;
                    setCustomLabel(parts.join('/'));
                  }
                }}
              >
                {cat}
              </button>
            );
          }}
        </For>
      </div>
    </Show>

    <div class="flex gap-2">
      <input
        type="text"
        value={customLabel()}
        onInput={(e) => setCustomLabel(e.currentTarget.value)}
        placeholder="Enter custom label..."
        class="input flex-1"
        autofocus
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            handleDecision('customlabel', customLabel());
          } else if (e.key === 'Escape') {
            setShowCustomLabel(false);
            setCustomLabel('');
          }
        }}
      />
      <button
        onClick={() => handleDecision('customlabel', customLabel())}
        class="btn-primary"
        disabled={!customLabel()}
      >
        Apply
      </button>
      <button
        onClick={() => {
          setShowCustomLabel(false);
          setCustomLabel('');
        }}
        class="btn-secondary"
      >
        Cancel
      </button>
    </div>
  </div>
</Show>
```

**Step 3: Commit**

```bash
git add ui/src/components/views/ReviewView.tsx
git commit -m "feat: add category quick-change dropdown to custom label input"
```

---

### Task 8: Make overlap analysis section collapsible

**Files:**
- Modify: `ui/src/components/views/FiltersView.tsx:283-461`

**Step 1: Add collapsed state signal**

Add near the other signals at the top of `FiltersView` (after line 15):

```typescript
const [overlapExpanded, setOverlapExpanded] = createSignal(false);
```

**Step 2: Replace the overlap analysis section**

Replace lines 283-461 with a collapsible wrapper. The header is always visible; the content toggles:

```tsx
{/* Overlap Analysis */}
<Show when={analysis()}>
  <div class="card p-6">
    {/* Clickable header - always visible */}
    <button
      class="w-full flex items-center justify-between mb-0"
      classList={{ 'mb-4': overlapExpanded() }}
      onClick={() => setOverlapExpanded(!overlapExpanded())}
    >
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white">
        Overlap Analysis
      </h3>
      <div class="flex items-center gap-3">
        {/* Summary counts - always visible */}
        <div class="flex gap-2 text-sm">
          <Show when={analysis()!.error_count > 0}>
            <span class="px-2 py-0.5 rounded bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300">
              {analysis()!.error_count} errors
            </span>
          </Show>
          <Show when={analysis()!.warning_count > 0}>
            <span class="px-2 py-0.5 rounded bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-300">
              {analysis()!.warning_count} warnings
            </span>
          </Show>
          <Show when={analysis()!.info_count > 0}>
            <span class="px-2 py-0.5 rounded bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300">
              {analysis()!.info_count} info
            </span>
          </Show>
        </div>
        {/* Chevron */}
        <svg
          class="w-5 h-5 text-gray-400 transition-transform"
          classList={{ 'rotate-180': overlapExpanded() }}
          fill="none" stroke="currentColor" viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
        </svg>
      </div>
    </button>

    {/* Collapsible content */}
    <Show when={overlapExpanded()}>
      {/* Controls row */}
      <div class="flex items-center gap-4 mb-4">
        <button
          onClick={() => setShowHiddenModal(true)}
          class="text-sm px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600 transition-colors"
        >
          Manage Hidden ({hiddenFilters().length})
        </button>

        <div class="flex items-center gap-2 bg-gray-100 dark:bg-gray-700 rounded-lg p-1">
          <button
            onClick={() => handleModeToggle(false)}
            class="text-sm px-3 py-1.5 rounded-md transition-colors"
            classList={{
              'bg-white dark:bg-gray-600 text-gray-900 dark:text-white shadow-sm': !autoManagedOnly(),
              'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white': autoManagedOnly(),
            }}
          >
            All Overlaps
          </button>
          <button
            onClick={() => handleModeToggle(true)}
            class="text-sm px-3 py-1.5 rounded-md transition-colors"
            classList={{
              'bg-white dark:bg-gray-600 text-gray-900 dark:text-white shadow-sm': autoManagedOnly(),
              'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white': !autoManagedOnly(),
            }}
          >
            Auto-Managed Only
          </button>
        </div>

        <Show when={theoreticalCount() > 0}>
          <button
            onClick={() => setShowTheoretical(!showTheoretical())}
            class="text-sm px-3 py-1.5 rounded-lg transition-colors"
            classList={{
              'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300': showTheoretical(),
              'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white': !showTheoretical(),
            }}
          >
            {showTheoretical() ? 'Hide' : 'Show'} Theoretical ({theoreticalCount()})
          </button>
        </Show>
      </div>

      <p class="text-sm text-gray-500 dark:text-gray-400 mb-4">
        {analysis()!.summary}
      </p>

      <Show when={analysis()!.error_count > 0 || analysis()!.warning_count > 0 || (showTheoretical() && theoreticalCount() > 0)}>
        <div class="space-y-3">
          <For each={visibleConflicts().filter(c =>
            c.severity !== 'Info' || (showTheoretical() && c.conflict_type === 'Theoretical Overlap')
          )}>
            {(conflict) => {
              const isTheoretical = conflict.conflict_type === 'Theoretical Overlap';
              return (
              <div
                class="p-4 rounded-lg border relative"
                classList={{
                  'bg-red-50 dark:bg-red-900/30 border-red-200 dark:border-red-800': conflict.severity === 'Error',
                  'bg-yellow-50 dark:bg-yellow-900/30 border-yellow-200 dark:border-yellow-800': conflict.severity === 'Warning' && !isTheoretical,
                  'bg-blue-50 dark:bg-blue-900/20 border-blue-200 dark:border-blue-800': isTheoretical,
                }}
              >
                <div class="flex items-start gap-3">
                  <span
                    class="text-sm font-medium px-2 py-0.5 rounded flex-shrink-0"
                    classList={{
                      'bg-red-100 text-red-700 dark:bg-red-800 dark:text-red-200': conflict.severity === 'Error',
                      'bg-yellow-100 text-yellow-700 dark:bg-yellow-800 dark:text-yellow-200': conflict.severity === 'Warning' && !isTheoretical,
                      'bg-blue-100 text-blue-700 dark:bg-blue-800 dark:text-blue-200': isTheoretical,
                    }}
                  >
                    {isTheoretical ? 'Theoretical' : conflict.severity}
                  </span>
                  <div class="flex-1 min-w-0">
                    <p class="text-sm font-medium text-gray-900 dark:text-white">
                      {conflict.conflict_type}
                    </p>
                    <p class="text-sm text-gray-600 dark:text-gray-400 mt-1">
                      {conflict.description}
                    </p>

                    <div class="mt-3 space-y-2 text-xs">
                      <div class="p-2 bg-white dark:bg-gray-800 rounded border border-gray-200 dark:border-gray-600 relative group">
                        <p class="font-medium text-gray-700 dark:text-gray-300 mb-1">Filter A:</p>
                        <p class="font-mono text-gray-600 dark:text-gray-400 break-all">
                          {conflict.filter_a_query || conflict.filter_a_name}
                        </p>
                        <Show when={conflict.filter_a_label}>
                          <p class="text-gray-500 dark:text-gray-500 mt-1">
                            Label: <span class="font-medium">{conflict.filter_a_label}</span>
                          </p>
                        </Show>
                        <button
                          onClick={() => handleHideFilter(
                            conflict.filter_a_id,
                            conflict.filter_a_name,
                            conflict.filter_a_query,
                            conflict.filter_a_label
                          )}
                          disabled={hidingInProgress() === conflict.filter_a_id}
                          class="absolute top-2 right-2 text-xs px-2 py-1 rounded bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors opacity-0 group-hover:opacity-100"
                          title="Hide this filter from analysis"
                        >
                          {hidingInProgress() === conflict.filter_a_id ? '...' : 'Hide'}
                        </button>
                      </div>
                      <div class="p-2 bg-white dark:bg-gray-800 rounded border border-gray-200 dark:border-gray-600 relative group">
                        <p class="font-medium text-gray-700 dark:text-gray-300 mb-1">Filter B:</p>
                        <p class="font-mono text-gray-600 dark:text-gray-400 break-all">
                          {conflict.filter_b_query || conflict.filter_b_name}
                        </p>
                        <Show when={conflict.filter_b_label}>
                          <p class="text-gray-500 dark:text-gray-500 mt-1">
                            Label: <span class="font-medium">{conflict.filter_b_label}</span>
                          </p>
                        </Show>
                        <button
                          onClick={() => handleHideFilter(
                            conflict.filter_b_id,
                            conflict.filter_b_name,
                            conflict.filter_b_query,
                            conflict.filter_b_label
                          )}
                          disabled={hidingInProgress() === conflict.filter_b_id}
                          class="absolute top-2 right-2 text-xs px-2 py-1 rounded bg-gray-200 dark:bg-gray-600 text-gray-600 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-500 transition-colors opacity-0 group-hover:opacity-100"
                          title="Hide this filter from analysis"
                        >
                          {hidingInProgress() === conflict.filter_b_id ? '...' : 'Hide'}
                        </button>
                      </div>
                    </div>

                    <Show when={conflict.suggestions.length > 0}>
                      <div class="mt-3">
                        <p class="text-xs text-gray-500 dark:text-gray-500">Suggestions:</p>
                        <ul class="text-xs text-gray-600 dark:text-gray-400 mt-1 space-y-1">
                          <For each={conflict.suggestions}>
                            {(suggestion) => <li>- {suggestion}</li>}
                          </For>
                        </ul>
                      </div>
                    </Show>
                  </div>
                </div>
              </div>
            );}}
          </For>
        </div>
      </Show>

      <Show when={analysis()!.error_count === 0 && analysis()!.warning_count === 0 && (!showTheoretical() || theoreticalCount() === 0)}>
        <div class="text-center py-8 text-gray-500 dark:text-gray-400">
          <p>No conflicts found.</p>
          <Show when={theoreticalCount() > 0}>
            <p class="text-sm mt-2">
              ({theoreticalCount()} theoretical overlap{theoreticalCount() !== 1 ? 's' : ''} hidden)
            </p>
          </Show>
        </div>
      </Show>
    </Show>
  </div>
</Show>
```

**Step 3: Commit**

```bash
git add ui/src/components/views/FiltersView.tsx
git commit -m "feat: make overlap analysis section collapsible, collapsed by default"
```

---

### Task 9: Add apply filters progress indicator

**Files:**
- Modify: `ui/src/components/views/FiltersView.tsx:1-4` (imports)
- Modify: `ui/src/components/views/FiltersView.tsx:130-160` (handleApply)
- Modify: `ui/src/components/views/FiltersView.tsx:186-203` (button area)

**Step 1: Add imports and signal**

Update the import at line 1 to include `onCleanup`:

```typescript
import { Component, createSignal, onMount, onCleanup, Show, For } from 'solid-js';
```

Add import for event listener (after line 3):

```typescript
import { onFilterProgress } from '../../lib/events';
```

Add signal near the other signals (after line 15):

```typescript
const [applyProgress, setApplyProgress] = createSignal<{ current: number; total: number; name?: string } | null>(null);
const [isApplying, setIsApplying] = createSignal(false);
```

**Step 2: Update `handleApply` to subscribe to progress events**

Replace the `handleApply` function (lines 130-160):

```typescript
const handleApply = async (dryRun: boolean) => {
  setApplyResult(null);
  setError(null);
  setIsApplying(true);
  setApplyProgress(null);

  // Subscribe to progress events
  let unlisten: (() => void) | null = null;
  try {
    unlisten = await onFilterProgress((progress) => {
      setApplyProgress({
        current: progress.current,
        total: progress.total,
        name: progress.filter_name,
      });
    });

    const result = await api.applyFilters(dryRun);

    if (result.success) {
      setApplyResult({
        success: true,
        message: dryRun
          ? `Dry run complete: Would create ${result.created} filters`
          : `Successfully created ${result.created} filters`,
      });
    } else {
      setApplyResult({
        success: false,
        message: `Created ${result.created} filters, ${result.failed} failed`,
      });
      if (result.errors.length > 0) {
        setError(result.errors.join('\n'));
      }
    }

    if (!dryRun) {
      await loadFilters();
    }
  } catch (e) {
    setError(e instanceof Error ? e.message : String(e));
  } finally {
    if (unlisten) unlisten();
    setIsApplying(false);
    setApplyProgress(null);
  }
};
```

**Step 3: Update buttons to disable during apply and show progress**

Replace the button area (lines 188-203):

```tsx
<div class="flex gap-2">
  <Show when={!isApplying()} fallback={
    <div class="flex items-center gap-3">
      <div class="animate-spin w-5 h-5 border-2 border-primary-200 border-t-primary-600 rounded-full" />
      <span class="text-sm text-gray-600 dark:text-gray-400">
        <Show when={applyProgress()} fallback="Preparing...">
          Creating filter {applyProgress()!.current}/{applyProgress()!.total}
          <Show when={applyProgress()!.name}>
            : {applyProgress()!.name}
          </Show>
        </Show>
      </span>
    </div>
  }>
    <button
      onClick={() => handleApply(true)}
      disabled={isLoading()}
      class="btn-secondary"
    >
      Dry Run
    </button>
    <button
      onClick={() => handleApply(false)}
      disabled={isLoading()}
      class="btn-primary"
    >
      Apply Filters
    </button>
  </Show>
</div>
```

**Step 4: Commit**

```bash
git add ui/src/components/views/FiltersView.tsx
git commit -m "feat: show progress indicator when applying filters"
```

---

### Task 10: Final verification

**Step 1: Full Rust test suite**

Run: `cargo test 2>&1`
Expected: All tests pass

**Step 2: TypeScript type check**

Run: `cd /Users/victorb/code/gmail-cleanup/ui && npx tsc --noEmit 2>&1`
Expected: Clean

**Step 3: Build check**

Run: `cd /Users/victorb/code/gmail-cleanup && cargo tauri build --debug 2>&1`
Expected: Builds successfully (or use `cargo check` if build takes too long)
