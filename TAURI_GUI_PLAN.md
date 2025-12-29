# Gmail Cleanup - Tauri GUI Implementation Plan

## Overview

Add a Tauri 2.0 desktop GUI as an alternative to the CLI for reviewing email filters, visualizing coverage, and detecting filter overlaps. The GUI will be a separate binary (`gmail-cleanup-gui`) that shares the existing `gmail_automation` library.

**Key Features:**
- Filter visualization (tree view, diff between current/proposed)
- Coverage analysis (bar charts, Venn diagrams, heatmap matrix)
- AST-based filter overlap detection (no example emails required)
- Cluster review interface (replacing crossterm terminal UI)
- Real-time progress streaming via Tauri events

**Technology Stack:**
- Backend: Tauri 2.0 + existing `gmail_automation` library
- Frontend: SolidJS + Tailwind CSS + Chart.js
- Visualization: Custom SVG for Venn diagrams, Canvas for heatmaps

---

## Project Structure

```
gmail-cleanup/
├── src/                          # Existing library (unchanged)
├── src-tauri/                    # NEW: Tauri backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── icons/
│   └── src/
│       ├── main.rs               # Tauri entry point
│       ├── commands/             # Tauri command modules
│       │   ├── mod.rs
│       │   ├── auth.rs
│       │   ├── scan.rs
│       │   ├── filters.rs
│       │   ├── clusters.rs
│       │   └── analysis.rs
│       ├── state.rs              # App state wrapper
│       └── events.rs             # Event emission helpers
├── ui/                           # NEW: SolidJS frontend
│   ├── package.json
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html
│   └── src/
│       ├── index.tsx
│       ├── App.tsx
│       ├── components/
│       ├── stores/
│       ├── hooks/
│       └── types/
└── Cargo.toml                    # Add workspace config
```

---

## Implementation Phases

### Phase 1: Foundation & AST Overlap Detection
**Goal:** Set up Tauri project structure and implement core filter analysis

#### 1.1 AST-Based Filter Overlap System
**Files to create:**
- `src/filter_ast.rs` - AST data structures for filter expressions
- `src/filter_overlap.rs` - Overlap detection algorithms

**Key types:**
```rust
// Filter expression AST
pub enum FromClause {
    Domain(DomainPattern),      // *@domain.com
    SpecificSender(EmailPattern), // user@domain.com
}

pub struct FilterExpr {
    pub from_clause: Option<FromClause>,
    pub subject_clause: Option<SubjectClause>,
    pub exclusions: Vec<ExclusionClause>,
}

// Overlap analysis results
pub enum PatternRelation {
    Disjoint, Identical, Subsumes, SubsumedBy, Overlaps { description: String }
}

pub struct FilterConflict {
    pub filter_a_id: String,
    pub filter_b_id: String,
    pub conflict_type: ConflictType,  // Overlap, Redundancy, ExclusionConflict, LabelConflict
    pub severity: ConflictSeverity,   // Info, Warning, Error
    pub description: String,
    pub resolution_suggestions: Vec<String>,
}
```

**Detection capabilities:**
- Domain vs specific sender overlap (`*@github.com` subsumes `bot@github.com`)
- Exclusion resolution (exclusions that negate overlaps)
- Subject keyword intersection
- Label/archive setting conflicts
- Redundancy detection (filter A completely covers filter B)

**Files to modify:**
- `src/lib.rs` - Export new modules
- `src/filter_manager.rs` - Integrate with `deduplicate_filters()`

#### 1.2 Tauri Project Setup
**Files to create:**
- `src-tauri/Cargo.toml`:
```toml
[package]
name = "gmail-cleanup-gui"
version = "0.1.0"
edition = "2021"

[dependencies]
gmail-automation = { path = ".." }
tauri = { version = "2", features = ["devtools"] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
parking_lot = "0.12"
```

- `src-tauri/tauri.conf.json` - Window config, build settings
- `src-tauri/capabilities/default.json` - Tauri 2.0 permissions
- `src-tauri/src/main.rs` - App builder with command registration
- `src-tauri/src/state.rs` - `AppState` struct wrapping `gmail_automation` types

#### 1.3 Frontend Scaffolding
**Files to create:**
- `ui/package.json` with SolidJS, Tailwind, Vite, Chart.js
- `ui/vite.config.ts`
- `ui/src/index.tsx` - Entry point
- `ui/src/App.tsx` - Main layout
- `ui/src/lib/api.ts` - Type-safe Tauri command wrappers
- `ui/src/lib/events.ts` - Event listeners

---

### Phase 2: Core Commands & Cluster Review
**Goal:** Implement backend commands and cluster review UI

#### 2.1 Authentication Commands
**File:** `src-tauri/src/commands/auth.rs`
- `check_auth_status()` - Return auth state
- `authenticate(credentials_path)` - Trigger OAuth flow

#### 2.2 Scanning Commands
**File:** `src-tauri/src/commands/scan.rs`
- `scan_emails(period_days)` - Start email scan
- Emit `scan:progress` events for real-time updates

#### 2.3 Cluster Commands
**File:** `src-tauri/src/commands/clusters.rs`
- `get_clusters(min_emails)` - Return clusters for review
- `submit_cluster_decision(cluster_id, action, label, archive)`
- `undo_last_decision()`
- `apply_decisions(dry_run)`

#### 2.4 Cluster Review UI
**Components:**
- `ClusterList.tsx` - Left panel with cluster list
- `ClusterDetail.tsx` - Center panel with cluster info
- `DecisionButtons.tsx` - Accept/Reject/Skip/Custom actions
- `SampleEmails.tsx` - Email preview cards
- `ExistingFilterDiff.tsx` - Side-by-side comparison

**Keyboard shortcuts (matching CLI):**
| Key | Action |
|-----|--------|
| Y/Enter | Accept |
| N | Reject |
| S | Skip |
| D | Delete |
| E | Exclude |
| L | Custom label |
| A | Toggle archive |
| U | Undo |

---

### Phase 3: Filter Visualization
**Goal:** Tree view and diff visualization for filters

#### 3.1 Filter Commands
**File:** `src-tauri/src/commands/filters.rs`
- `get_existing_filters()` - Fetch from Gmail
- `get_proposed_filters()` - From current session
- `compare_filters()` - Generate diff structure
- `create_filter(filter)`, `delete_filter(id)`

#### 3.2 Filter Tree Component
**Components:**
- `FilterTree.tsx` - Hierarchical tree (AutoManaged/receipts/amazon)
- `FilterTreeNode.tsx` - Expandable node with badges
- `FilterDiff.tsx` - Side-by-side current vs proposed
- `FilterCard.tsx` - Individual filter details

**Visual indicators:**
- `[=]` Unchanged
- `[+]` New filter
- `[~]` Modified (label or archive changed)
- `[-]` To be deleted (orphaned)

---

### Phase 4: Coverage Analysis & Visualizations
**Goal:** Comprehensive coverage analysis with all visualization types

#### 4.1 Analysis Commands
**File:** `src-tauri/src/commands/analysis.rs`
- `analyze_filter_overlaps(filters)` - AST-based analysis
- `get_filter_coverage()` - Per-filter email counts
- `get_uncovered_emails()` - Emails matching no filter

#### 4.2 Bar Charts & Tables
**Components:**
- `CoverageChart.tsx` - Chart.js bar chart (filter → email count)
- `CoverageTable.tsx` - Sortable table with drill-down

#### 4.3 Venn Diagrams
**Component:** `OverlapVenn.tsx`
- Custom SVG rendering for 2-3 filter overlaps
- Interactive (click region to see emails)
- Shows overlap count in intersection

#### 4.4 Heatmap Matrix
**Component:** `OverlapMatrix.tsx`
- Canvas-based grid (filter × filter)
- Color intensity = overlap size
- Hover for details

#### 4.5 Overlap Resolution UI
**Component:** `OverlapSuggestions.tsx`
- Display detected conflicts from AST analysis
- Show resolution suggestions
- One-click fixes (add exclusion, merge filters)

---

### Phase 5: Polish & Integration
**Goal:** Final refinements and testing

#### 5.1 Progress View
- `ProgressBar.tsx` - Multi-phase progress
- `PhaseIndicator.tsx` - Visual pipeline stages
- `LiveStats.tsx` - Real-time counts
- Event-driven updates from backend

#### 5.2 Settings & Configuration
- Config editor (period_days, label_prefix, etc.)
- Theme toggle (dark/light)
- Persist window size/position

#### 5.3 Accessibility
- ARIA labels throughout
- Full keyboard navigation
- Screen reader announcements for progress
- High contrast theme option

#### 5.4 Testing
- Unit tests for AST overlap detection
- Integration tests for Tauri commands
- E2E tests for critical flows

---

## Critical Files Reference

### Existing files to modify:
| File | Changes |
|------|---------|
| `src/lib.rs` | Export `filter_ast`, `filter_overlap` modules |
| `src/filter_manager.rs` | Integrate AST analysis into `deduplicate_filters()` |
| `Cargo.toml` | Add workspace configuration |

### Existing files to understand (read-only reference):
| File | Purpose |
|------|---------|
| `src/interactive.rs` | `EmailCluster`, `ClusterDecision`, `DecisionAction` - replicate in GUI |
| `src/models.rs` | `FilterRule`, `MessageMetadata`, `Classification` - serialize for frontend |
| `src/cli.rs` | `run_pipeline()`, `Report` structure - adapt for GUI |
| `src/client.rs` | `ExistingFilterInfo`, `is_auto_managed()` - filter comparison |
| `src/filter_manager.rs` | `build_gmail_query_static()` - display filter queries |

### New files to create:
| File | Purpose |
|------|---------|
| `src/filter_ast.rs` | AST data structures |
| `src/filter_overlap.rs` | Overlap detection engine |
| `src-tauri/src/main.rs` | Tauri app entry |
| `src-tauri/src/state.rs` | App state management |
| `src-tauri/src/commands/*.rs` | Command implementations |
| `ui/src/components/**/*.tsx` | SolidJS components |

---

## Dependencies to Add

### src-tauri/Cargo.toml
```toml
tauri = "2"
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
parking_lot = "0.12"
```

### ui/package.json
```json
{
  "dependencies": {
    "solid-js": "^1.8",
    "@tauri-apps/api": "^2",
    "chart.js": "^4",
    "solid-chartjs": "^1"
  },
  "devDependencies": {
    "vite": "^5",
    "vite-plugin-solid": "^2",
    "tailwindcss": "^3",
    "typescript": "^5"
  }
}
```

---

## Detailed Component Designs

### Cluster Review View (Wireframe)
```
┌──────────────────────────────────────────────────────────────────────────────┐
│  gmail-cleanup > Review Clusters                    [12/47]  [Undo] [Finish] │
├────────────────┬───────────────────────────────────┬─────────────────────────┤
│ CLUSTERS       │ CLUSTER DETAIL                    │ ACTIONS SUMMARY         │
│ ──────────     │ ──────────────                    │ ───────────────         │
│                │                                   │                         │
│ ☑ github.com   │  *@linkedin.com (42 emails)       │  Accepted:    8         │
│   (156 emails) │  ═══════════════════════════════  │  Rejected:    3         │
│ ☑ linkedin.com │                                   │  Skipped:     0         │
│   (42 emails)  │  PROPOSED FILTER                  │  Remaining:  36         │
│ ☐ amazon.com   │  ┌─────────────────────────────┐  │                         │
│   (89 emails)  │  │ Query: from:(*@linkedin.com)│  │  ───────────────        │
│ ☐ newsletter   │  │ Label: [AutoManaged/notif▼] │  │                         │
│   @medium.com  │  │ Archive: [☑]                │  │  [U] Undo last          │
│   (23 emails)  │  └─────────────────────────────┘  │                         │
│ ⚠ orphaned-1   │                                   │  Keyboard:              │
│   (0 emails)   │  ⚠ EXISTING FILTER DETECTED       │  Y - Accept             │
│                │  ┌─────────────────────────────┐  │  N - Reject             │
│ Filter: [All▼] │  │ Current  │ Proposed         │  │  S - Skip               │
│ Search: [____] │  │──────────┼─────────────────│  │  A - Toggle archive     │
│                │  │ Label:   │                  │  │  L - Custom label       │
│                │  │ social   │ AutoManaged/notif│  │  D - Delete filter      │
│                │  │ Archive: │                  │  │  E - Exclude forever    │
│                │  │ No       │ Yes              │  │  ? - Help               │
│                │  └─────────────────────────────┘  │                         │
│                │                                   │                         │
│                │  SAMPLE EMAILS                    │                         │
│                │  ─────────────                    │                         │
│                │  • "John viewed your profile"     │                         │
│                │    Dec 15, 2025                   │                         │
│                │  • "New job matches for you"      │                         │
│                │    Dec 14, 2025                   │                         │
│                │  • "Weekly digest: 5 connections" │                         │
│                │    Dec 12, 2025                   │                         │
│                │                                   │                         │
├────────────────┴───────────────────────────────────┴─────────────────────────┤
│  [Y Accept]  [N Reject]  [S Skip]  [D Delete]  [E Exclude]  [L Label]        │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Filter Visualization View (Wireframe)
```
┌──────────────────────────────────────────────────────────────────────────────┐
│  gmail-cleanup > Filter Visualization          [Tree] [List] [Coverage]      │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  CURRENT FILTERS                    PROPOSED CHANGES                         │
│  ═══════════════                    ════════════════                         │
│                                                                              │
│  ▼ AutoManaged                      ▼ AutoManaged                            │
│    ├─ receipts                        ├─ receipts                            │
│    │  ├─ amazon-com (89)              │  ├─ amazon-com (89)        [=]       │
│    │  ├─ ebay-com (34)                │  ├─ ebay-com (34)          [=]       │
│    │  └─ paypal-com (67)              │  ├─ paypal-com (67)        [=]       │
│    │                                  │  └─ stripe-com (23)        [+] NEW   │
│    ├─ newsletters                     ├─ newsletters                         │
│    │  ├─ medium-com (156)             │  ├─ medium-com (156)       [=]       │
│    │  └─ substack-com (78)            │  └─ substack-com (78)      [=]       │
│    │                                  │                                      │
│    ├─ notifications                   ├─ notifications                       │
│    │  ├─ github-com (234)   ───────>  │  ├─ github-com (234)       [~] MOD   │
│    │  │  [archive: NO]                │  │  [archive: YES]                   │
│    │  └─ linkedin-com (42)            │  └─ linkedin-com (42)      [=]       │
│    │                                  │                                      │
│    └─ orphaned-filter-xyz   ───────>  │                            [-] DEL   │
│       [no matching emails]            │                                      │
│                                                                              │
│  ──────────────────────────────────────────────────────────────────────────  │
│  Legend: [=] Unchanged  [+] New  [~] Modified  [-] Deleted                   │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  Summary: 2 new filters, 1 modified, 1 deleted                         │  │
│  │           Total emails affected: 257                                   │  │
│  │                                                                        │  │
│  │  [Apply All Changes]     [Apply Selected]     [Discard]               │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Coverage Analysis View (Wireframe)
```
┌──────────────────────────────────────────────────────────────────────────────┐
│  gmail-cleanup > Coverage Analysis                                           │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  COVERAGE OVERVIEW                                                           │
│  ═════════════════                                                           │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │                                                                        │  │
│  │  Total: 3,542 emails                                                   │  │
│  │  ██████████████████████████████████████░░░░░░░░░░ 78.0%               │  │
│  │  Covered: 2,763    Uncovered: 779                                      │  │
│  │                                                                        │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                                                              │
│  FILTER BREAKDOWN                                OVERLAP DETECTION           │
│  ════════════════                                ═════════════════           │
│                                                                              │
│  Filter              Emails   %      Overlap    ┌───────────────────────┐   │
│  ─────────────────────────────────────────────  │                       │   │
│  github.com          234    6.6%     12         │    ┌─────┐            │   │
│  amazon.com           89    2.5%      3         │   /       \    github │   │
│  linkedin.com         42    1.2%      0         │  │   12    │ ←overlap │   │
│  medium.com          156    4.4%     45         │  │    ╲   ╱           │   │
│  substack.com         78    2.2%     45         │   ╲   ╲ ╱    medium  │   │
│  paypal.com           67    1.9%      0         │    ╲───╳────╱         │   │
│  ...                                            │     substack          │   │
│                                                 └───────────────────────┘   │
│  ⚠ Overlaps Detected:                                                       │
│  ────────────────────                                                        │
│  • medium.com ∩ substack.com: 45 emails (newsletters with multiple senders) │
│    Suggestion: Merge into single "newsletters" filter                        │
│                                                                              │
│  • github.com ∩ notifications@github.com: 12 emails                          │
│    Suggestion: Make github.com exclude notifications@github.com             │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  [View Uncovered Emails]  [Resolve Overlaps]  [Export Report]          │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## AST Overlap Detection - Detailed Algorithm

### Core Data Structures

```rust
// src/filter_ast.rs

use serde::{Deserialize, Serialize};

/// Root AST node representing a complete filter expression
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterExpr {
    pub from_clause: Option<FromClause>,
    pub subject_clause: Option<SubjectClause>,
    pub exclusions: Vec<ExclusionClause>,
}

/// FROM clause patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromClause {
    Domain(DomainPattern),
    SpecificSender(EmailPattern),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainPattern {
    pub domain: String,           // e.g., "github.com"
    pub include_subdomains: bool, // true for *@*.github.com
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmailPattern {
    pub local_part: String,  // e.g., "noreply"
    pub domain: String,      // e.g., "github.com"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubjectClause {
    pub keywords: Vec<String>,
    pub match_mode: SubjectMatchMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SubjectMatchMode {
    Any,  // keyword1 OR keyword2
    All,  // keyword1 AND keyword2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExclusionClause {
    pub pattern: FromClause,
}
```

### Overlap Detection Algorithm

```rust
// src/filter_overlap.rs

impl FilterOverlapAnalyzer {
    /// Analyze relationship between two FROM clauses
    pub fn analyze_from_relation(a: &FromClause, b: &FromClause) -> PatternRelation {
        match (a, b) {
            // Domain vs Domain
            (FromClause::Domain(da), FromClause::Domain(db)) => {
                if da.domain.eq_ignore_ascii_case(&db.domain) {
                    PatternRelation::Identical
                } else if is_subdomain(&da.domain, &db.domain) && db.include_subdomains {
                    PatternRelation::SubsumedBy
                } else if is_subdomain(&db.domain, &da.domain) && da.include_subdomains {
                    PatternRelation::Subsumes
                } else {
                    PatternRelation::Disjoint
                }
            }

            // Specific sender vs Domain
            (FromClause::SpecificSender(email), FromClause::Domain(domain)) => {
                if email.domain.eq_ignore_ascii_case(&domain.domain) {
                    PatternRelation::SubsumedBy  // Domain catches all, including this sender
                } else {
                    PatternRelation::Disjoint
                }
            }

            // Domain vs Specific sender
            (FromClause::Domain(domain), FromClause::SpecificSender(email)) => {
                if email.domain.eq_ignore_ascii_case(&domain.domain) {
                    PatternRelation::Subsumes  // Domain catches all, including this sender
                } else {
                    PatternRelation::Disjoint
                }
            }

            // Specific sender vs Specific sender
            (FromClause::SpecificSender(a), FromClause::SpecificSender(b)) => {
                if a.full_address().eq_ignore_ascii_case(&b.full_address()) {
                    PatternRelation::Identical
                } else {
                    PatternRelation::Disjoint
                }
            }
        }
    }

    /// Check if exclusions resolve an overlap
    pub fn exclusions_resolve_overlap(filter_a: &FilterExpr, filter_b: &FilterExpr) -> bool {
        // Check if filter_a's exclusions exclude filter_b's positive matches
        for exclusion in &filter_a.exclusions {
            if let Some(ref from_b) = filter_b.from_clause {
                let relation = Self::analyze_from_relation(&exclusion.pattern, from_b);
                if matches!(relation, PatternRelation::Subsumes | PatternRelation::Identical) {
                    return true;  // Exclusion negates the overlap
                }
            }
        }
        false
    }
}
```

---

## Tauri Command Examples

### Authentication Command
```rust
// src-tauri/src/commands/auth.rs

#[derive(serde::Serialize)]
pub struct AuthStatus {
    pub authenticated: bool,
    pub email: Option<String>,
}

#[tauri::command]
pub async fn check_auth_status(
    state: State<'_, AppState>,
) -> Result<AuthStatus, String> {
    let token_path = state.token_path();
    if !token_path.exists() {
        return Ok(AuthStatus { authenticated: false, email: None });
    }

    let hub = gmail_automation::auth::initialize_gmail_hub(
        &state.credentials_path(),
        &token_path,
    ).await.map_err(|e| e.to_string())?;

    let (_, profile) = hub.users()
        .get_profile("me")
        .doit()
        .await
        .map_err(|e| e.to_string())?;

    Ok(AuthStatus {
        authenticated: true,
        email: profile.email_address,
    })
}
```

### Scanning with Progress Events
```rust
// src-tauri/src/commands/scan.rs

#[derive(Clone, serde::Serialize)]
pub struct ScanProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
}

#[tauri::command]
pub async fn scan_emails(
    app: AppHandle,
    period_days: u32,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let client = state.get_client().await?;

    // Emit progress event
    app.emit("scan:progress", ScanProgress {
        phase: "listing".into(),
        current: 0,
        total: 0,
    }).ok();

    let message_ids = client.list_message_ids("").await
        .map_err(|e| e.to_string())?;

    let total = message_ids.len();

    // Fetch with progress callbacks
    for (i, id) in message_ids.iter().enumerate() {
        app.emit("scan:progress", ScanProgress {
            phase: "fetching".into(),
            current: i + 1,
            total,
        }).ok();
        // ... fetch message
    }

    Ok(total)
}
```

### Overlap Analysis Command
```rust
// src-tauri/src/commands/analysis.rs

#[tauri::command]
pub async fn analyze_filter_overlaps(
    state: State<'_, AppState>,
) -> Result<Vec<FilterConflict>, String> {
    let filters = state.get_all_filters().await?;

    let mut analyzer = FilterOverlapAnalyzer::new();
    let result = analyzer.analyze_all(filters);

    Ok(result.conflicts)
}
```

---

## Frontend API Layer

```typescript
// ui/src/lib/api.ts

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// Types
export interface AuthStatus {
  authenticated: boolean;
  email: string | null;
}

export interface ScanProgress {
  phase: 'listing' | 'fetching' | 'complete';
  current: number;
  total: number;
}

export interface ClusterView {
  id: string;
  sender_pattern: string;
  email_count: number;
  suggested_label: string;
  should_archive: boolean;
  sample_subjects: string[];
  has_existing_filter: boolean;
}

export interface FilterConflict {
  filter_a_id: string;
  filter_b_id: string;
  conflict_type: 'Overlap' | 'Redundancy' | 'LabelConflict' | 'ArchiveConflict';
  severity: 'Info' | 'Warning' | 'Error';
  description: string;
  suggestions: string[];
}

// Commands
export const checkAuthStatus = () => invoke<AuthStatus>('check_auth_status');
export const authenticate = (path: string) => invoke<AuthStatus>('authenticate', { credentialsPath: path });
export const scanEmails = (days: number) => invoke<number>('scan_emails', { periodDays: days });
export const getClusters = (min: number) => invoke<ClusterView[]>('get_clusters', { minEmails: min });
export const analyzeOverlaps = () => invoke<FilterConflict[]>('analyze_filter_overlaps');

// Events
export const onScanProgress = (cb: (p: ScanProgress) => void) =>
  listen<ScanProgress>('scan:progress', e => cb(e.payload));
```

---

## Success Criteria

1. **Functional GUI** that can complete full review workflow without CLI
2. **Filter overlap detection** identifies conflicts without example emails
3. **Coverage visualization** shows all three chart types (bar, Venn, heatmap)
4. **Keyboard efficiency** matches CLI (Y/N/S shortcuts work)
5. **Progress streaming** shows real-time scan/classification status
6. **Filter diff** clearly shows changes between current and proposed

---

## Sources

- [Tauri 2.0 Documentation](https://v2.tauri.app/)
- [Tauri Commands Guide](https://v2.tauri.app/develop/calling-rust/)
- [Tauri Frontend Configuration](https://v2.tauri.app/start/frontend/)
- [SolidJS Documentation](https://www.solidjs.com/)
- [Chart.js Documentation](https://www.chartjs.org/)
