# Plan: Fix Filter Application Failure in Tauri GUI

## Problem Statement
When applying filters in the GUI, all filters fail (e.g., "3/3 failed"). The errors are captured but not visible to the user in the UI.

## Root Cause Analysis
The GUI passes label **names** (e.g., "Newsletter") to the Gmail API, but Gmail expects label **IDs** (e.g., "Label_123abc").

**Flow of the bug:**
1. User accepts a cluster with `suggested_label = "AutoManaged/Newsletter"` (a name)
2. `submit_cluster_decision` stores `target_label = cluster.suggested_label` (the name)
3. `generate_proposed_filters` creates `FilterRule { target_label_id: label }` (still the name)
4. `apply_filters` calls Gmail API with `add_label_ids: ["AutoManaged/Newsletter"]`
5. Gmail API rejects it - "AutoManaged/Newsletter" is not a valid label ID

## Key Files

| File | Issue |
|------|-------|
| `src-tauri/src/commands/filters.rs:162-184` | `generate_proposed_filters` uses label name as ID |
| `src-tauri/src/commands/clusters.rs:201-207` | `GuiDecision.target_label` stores name, not ID |
| `src/client.rs:814-822` | Gmail API expects label IDs in `add_label_ids` |

## Implementation Plan

### Step 1: Add Label Resolution in `generate_proposed_filters`

Before creating `FilterRule` objects, resolve label names to IDs:

```rust
// In generate_proposed_filters, after getting client
let client = state.get_client().ok_or("Gmail client not initialized")?;

// Fetch labels and build name -> ID map
let labels = client.list_labels().await
    .map_err(|e| format!("Failed to fetch labels: {}", e))?;
let label_name_to_id: HashMap<String, String> = labels
    .into_iter()
    .map(|l| (l.name, l.id))
    .collect();

// When building FilterRule:
let label_id = label_name_to_id
    .get(&label)
    .cloned()
    .ok_or_else(|| format!("Label '{}' not found in Gmail", label))?;

let filter = FilterRule {
    // ...
    target_label_id: label_id,  // Now using actual ID
    // ...
};
```

### Step 2: Create Missing Labels

If a label doesn't exist, create it before filter creation:

```rust
let label_id = match label_name_to_id.get(&label) {
    Some(id) => id.clone(),
    None => {
        // Label doesn't exist, create it
        let new_id = client.create_label(&label).await
            .map_err(|e| format!("Failed to create label '{}': {}", label, e))?;
        new_id
    }
};
```

### Step 3: Surface Errors in UI

Currently errors are returned but may not be displayed. In the frontend filter application UI:

```tsx
// In the component that calls applyFilters
const result = await api.applyFilters(dryRun);
if (!result.success) {
    // Show errors to user
    setErrors(result.errors);
}
```

Check `ui/src/components/views/FiltersView.tsx` or wherever `applyFilters` is called to ensure errors are displayed.

### Step 4: Use Existing Label Infrastructure

`create_label` already exists in `src/client.rs:753`. There's also a `LabelManager` in `src/label_manager.rs` with `get_or_create_label` (line 151) which:
- Checks if label exists first
- Creates it if not
- Handles nested labels (e.g., "AutoManaged/Newsletter")

**Option A (simpler):** Use client directly:
```rust
let label_id = client.create_label(&label_name).await?;
```

**Option B (better):** Use LabelManager for proper hierarchy handling:
```rust
let mut label_manager = LabelManager::new(Box::new(client.clone()), label_prefix);
let label_id = label_manager.get_or_create_label(&label_name).await?;
```

## Files to Modify

| File | Changes |
|------|---------|
| `src-tauri/src/commands/filters.rs` | Add label name→ID resolution in `generate_proposed_filters` |
| `src/client.rs` | Add `create_label` method if missing |
| `ui/src/components/views/FiltersView.tsx` | Ensure errors are displayed to user |

## Alternative Approach

Instead of resolving at filter generation time, resolve in `apply_filters`:

**Pros:** Keeps `generate_proposed_filters` simple, resolution happens closer to API call
**Cons:** Errors happen later in the flow, user already committed to applying

**Recommendation:** Resolve in `generate_proposed_filters` so errors surface early and user can see valid proposed filters before applying.

## Testing Checklist

- [ ] Apply filter with existing label → should succeed
- [ ] Apply filter with new label (needs creation) → should create label then filter
- [ ] Apply filter with invalid characters in label name → should show error
- [ ] Errors from Gmail API are visible in UI
