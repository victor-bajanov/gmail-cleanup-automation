# Gmail Email Management Automation System
## Functional Design Specification

**Document Version:** 1.0  
**Date:** 24 November 2025  
**Author:** Victor Bajanov  
**Status:** Draft

---

## 1. Executive Summary

### 1.1 Purpose
This document defines the functional requirements and design specifications for an automated Gmail email management system that scans historical emails, intelligently classifies them, and creates organised filters and labels to maintain inbox hygiene.

### 1.2 Problem Statement
Many Gmail users accumulate thousands of unread emails, primarily commercial or automated communications they've subscribed to. Manual organisation is time-consuming and inconsistent. The lack of systematic filtering leads to important emails being buried among newsletters, notifications, and promotional content.

### 1.3 Solution Overview
An intelligent automation system that:
- Analyses the last 3 months of emails
- Classifies messages using rule-based and optional ML-based logic **during initial analysis only**
- Generates deterministic, Gmail-compatible filters based on classification patterns
- Creates a hierarchical, non-redundant label structure
- Applies generated filters and labels to Gmail account
- Applies labels retroactively to existing messages

**Important Design Principle:** AI/ML classification is used only during the initial filter generation phase to analyse patterns and create deterministic rules. Once filters are created, they run natively in Gmail using standard Gmail filter syntax—no ongoing AI processing is required.

### 1.4 Success Criteria
- Process 100% of emails from the specified time period
- Achieve >90% classification accuracy (validated by user sampling)
- Create <50 unique labels for typical users (avoiding label proliferation)
- Reduce inbox clutter by automatically archiving >80% of non-actionable emails
- Execute within API quota limits
- Complete initial scan and setup within 30 minutes for typical inboxes

---

## 2. System Architecture

### 2.0 Design Philosophy: Deterministic Filters

**Core Principle:** This system generates Gmail-native filters that work deterministically without requiring ongoing AI processing.

**Why This Matters:**
1. **Performance:** Filters run instantly in Gmail's infrastructure
2. **Reliability:** No dependency on external services or API availability
3. **Cost:** No ongoing LLM API costs after initial setup
4. **Privacy:** Email content never leaves Gmail after initial analysis
5. **Simplicity:** Users understand and can modify filter rules
6. **Longevity:** Filters continue working even if this tool is no longer maintained

**AI/ML Role:**
- **During Setup:** Analyse email patterns to generate smart filter rules
- **After Setup:** Not used—filters run purely in Gmail

**Example Flow:**
1. System analyses 100 emails from `newsletter@company.com`
2. LLM identifies patterns: sender domain, "Weekly Digest" in subject
3. System generates Gmail filter: `from:(*@company.com) subject:(weekly digest)`
4. Filter is created in Gmail
5. **Future emails matching this filter are auto-labelled by Gmail itself**

This approach combines AI intelligence with deterministic execution.

### 2.1 High-Level Architecture

```
┌─────────────────┐
│  User Interface │ (CLI or Web)
└────────┬────────┘
         │
┌────────▼────────────────────────────────────┐
│         Application Layer                    │
├──────────────────────────────────────────────┤
│  • Authentication Manager                    │
│  • Email Scanner                             │
│  • Classification Engine                     │
│  • Label Manager                             │
│  • Filter Manager                            │
│  • Analytics & Reporting                     │
└────────┬────────────────────────────────────┘
         │
┌────────▼────────┐
│   Gmail API     │
└─────────────────┘
```

### 2.2 Technology Stack

**Core Requirements:**
- Rust 1.70+ (2021 edition)
- `google-gmail1` crate for Gmail API
- `yup-oauth2` crate for OAuth2 authentication
- `tokio` for async runtime
- `reqwest` for HTTP client

**Optional Components:**
- `rusqlite` for local caching and state management
- `async-openai` / `anthropic-sdk` for ML-based classification
- `ratatui` (formerly tui-rs) for CLI interface
- `serde` and `serde_json` for serialisation
- `anyhow` and `thiserror` for error handling
- `tracing` for structured logging

---

## 3. Functional Requirements

### 3.1 Authentication & Authorisation

#### FR-1.1: OAuth2 Authentication
**Priority:** Must Have  
**Description:** System must authenticate with Gmail API using OAuth2.

**Requirements:**
- Support OAuth2 flow with appropriate scopes using `yup-oauth2`:
  - `https://www.googleapis.com/auth/gmail.readonly` (for scanning)
  - `https://www.googleapis.com/auth/gmail.labels` (for label management)
  - `https://www.googleapis.com/auth/gmail.settings.basic` (for filter creation)
  - `https://www.googleapis.com/auth/gmail.modify` (for applying labels)
- Store credentials securely in local file system using `tokio::fs`
- Refresh tokens automatically when expired via `yup-oauth2::ApplicationSecret`
- Handle authentication errors gracefully with proper error types

**Acceptance Criteria:**
- User can authenticate via browser-based OAuth2 flow
- Credentials persist between sessions in `~/.gmail-automation/token.json`
- Token refresh occurs automatically using `yup-oauth2::RefreshToken`
- Clear error messages for authentication failures using `thiserror`

#### FR-1.2: Credential Management
**Priority:** Must Have  
**Description:** Securely manage and store API credentials.

**Requirements:**
- Store OAuth2 tokens with restricted file permissions (0600)
- Store credentials separate from application code
- Support credential revocation and re-authentication
- Validate credential scopes match requirements

---

### 3.2 Email Scanning

#### FR-2.1: Historical Email Retrieval
**Priority:** Must Have  
**Description:** Retrieve all emails from the last 3 months for analysis.

**Requirements:**
- Calculate date range: current date minus 90 days
- Use `messages.list` with date filter: `after:YYYY/MM/DD`
- Retrieve message IDs for all matching messages
- Handle pagination with `nextPageToken`
- Support user-configurable time ranges (1 month, 3 months, 6 months, 1 year)

**Acceptance Criteria:**
- All message IDs from specified period are retrieved
- Pagination handles mailboxes with >10,000 messages
- Progress indicator shows retrieval status
- Time range can be specified via configuration

#### FR-2.2: Concurrent Message Retrieval
**Priority:** Must Have  
**Description:** Efficiently retrieve full message details using concurrent individual requests with rate limiting.

**Requirements:**
- Concurrent message retrieval with bounded parallelism (40-50 concurrent)
- Request format: `metadata` to minimise payload size
- Extract fields: `id`, `threadId`, `labelIds`, `payload.headers` (From, Subject, To, Date, List-Unsubscribe)
- Implement exponential backoff for rate limit errors
- Cache retrieved messages locally to avoid re-fetching

**Technical Specifications:**
- Concurrency limit: 40 requests (to stay under 250 units/sec)
- Retry logic: Exponential backoff starting at 100ms
- Maximum retries: 5 attempts
- Timeout: 30 seconds per request

**Note on Batching:** We use concurrent individual requests rather than native Gmail batch requests because:
- Quota consumption is identical (batched requests still count as n requests)
- Rate limiting (250 units/sec) is the bottleneck, not HTTP overhead
- HTTP/2 connection reuse eliminates most latency benefits of batching
- Individual requests provide better error handling and retry logic

**Acceptance Criteria:**
- Message details retrieved within API quota limits
- Rate limit errors handled without data loss
- Progress indicator shows processing status
- Failed requests are retried automatically

#### FR-2.3: Email Filtering
**Priority:** Must Have  
**Description:** Focus analysis on relevant emails, excluding spam and trash.

**Requirements:**
- Exclude messages with labels: `SPAM`, `TRASH`
- Optionally exclude messages with labels: `SENT`, `DRAFT`
- Allow user to specify additional exclusion criteria
- Support inclusion of specific folders/labels for analysis

**Acceptance Criteria:**
- Spam and trash messages are never processed
- User can configure exclusions via settings
- Exclusion logic is applied before batch retrieval

---

### 3.3 Email Classification

#### FR-3.1: Metadata Extraction
**Priority:** Must Have  
**Description:** Extract key metadata from each message for classification.

**Requirements:**
- Extract sender email address and domain
- Extract subject line
- Extract recipient list (To, CC)
- Identify reply/forward indicators in subject
- Extract list-unsubscribe headers if present
- Identify thread information (standalone vs conversation)

**Data Structure:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    pub message_id: String,
    pub thread_id: String,
    pub sender_email: String,
    pub sender_domain: String,
    pub sender_name: String,
    pub subject: String,
    pub recipients: Vec<String>,
    pub is_reply: bool,
    pub is_forward: bool,
    pub has_unsubscribe: bool,
    pub existing_labels: Vec<String>,
    pub date_received: DateTime<Utc>,
}
```

#### FR-3.2: Rule-Based Classification
**Priority:** Must Have  
**Description:** Classify emails using pattern matching and heuristics.

**Classification Rules:**

1. **Automated/Commercial Indicators:**
   - Sender patterns: `noreply@`, `no-reply@`, `notifications@`, `marketing@`, `newsletter@`, `info@`
   - Has list-unsubscribe header
   - Generic sender names: "Team", "Updates", "Notifications"

2. **Category Detection:**
   - **Newsletters:** Subject contains "newsletter", "digest", "roundup", "weekly", "monthly"
   - **Receipts/Orders:** Subject contains "receipt", "order confirmation", "invoice", "payment"
   - **Notifications:** Subject contains "notification", "alert", "reminder", "update"
   - **Social Media:** Domains from known social platforms (facebook.com, linkedin.com, twitter.com, etc.)
   - **Shipping:** Subject contains "shipped", "tracking", "delivery", "dispatched"
   - **Financial:** Domains from banks, payment processors; subjects about statements, transactions
   - **Travel:** Domains from airlines, hotels, booking platforms

3. **Priority Signals:**
   - **Keep in Inbox:** Direct personal emails (not from noreply addresses), emails in active threads
   - **Archive:** Automated notifications, newsletters, promotional content

**Acceptance Criteria:**
- Each email assigned at least one category
- Classification rules can be configured/extended
- Manual override mechanism for misclassifications
- Classification confidence score tracked

#### FR-3.3: Domain Clustering
**Priority:** Must Have  
**Description:** Group similar sender domains to create organised labels.

**Requirements:**
- Extract root domain from email addresses
- Identify common patterns in domain names
- Group subdomains under parent domains (e.g., `*.github.com` → `GitHub`)
- Detect domain variations (e.g., `marketing.company.com`, `news.company.com` → `Company`)
- Map domains to friendly names using built-in dictionary and web lookups

**Clustering Algorithm:**
1. Extract all unique sender domains
2. Normalise domains (remove common prefixes: mail., email., etc.)
3. Count email volume per domain
4. Group domains by second-level domain
5. Apply minimum threshold (e.g., >5 emails to create dedicated label)

**Acceptance Criteria:**
- Similar domains grouped logically
- Low-volume senders grouped under generic categories
- High-volume senders get dedicated labels
- Domain-to-label mapping is reviewable before application

#### FR-3.4: ML-Based Classification (Optional)
**Priority:** Should Have  
**Description:** Use LLM for intelligent, context-aware classification **during initial filter generation only**.

**Key Design Constraint:** The LLM is used to analyse email patterns and generate deterministic filter rules that Gmail can execute natively. The system does NOT require ongoing LLM access after initial setup.

**Requirements:**
- Support multiple LLM providers during initial analysis
- Classify based on email body content (first 500 words) to identify patterns
- Provide structured output with category and confidence
- **Generate Gmail-compatible filter criteria from LLM insights**
- Fall back to rule-based classification if LLM unavailable

**LLM Analysis Flow:**
1. Sample representative emails from each sender/pattern
2. Use LLM to identify common characteristics and patterns
3. **Extract deterministic rules** (sender domains, subject patterns, keywords)
4. Generate Gmail filter criteria based on identified patterns
5. Validate filters will work without ongoing LLM access

**LLM Prompt Structure:**
```rust
fn build_pattern_analysis_prompt(emails: &[MessageMetadata]) -> String {
    let email_samples = emails.iter()
        .take(5)
        .map(|e| format!("From: {}\nSubject: {}\n", e.sender_email, e.subject))
        .collect::<Vec<_>>()
        .join("\n");
    
    format!(r#"Analyse these email samples and extract DETERMINISTIC filter criteria 
that Gmail can use to automatically categorise similar future emails.

Email samples:
{}

Return JSON with Gmail-compatible filter rules:
{{
  "category": "Newsletter/Receipt/Notification/Marketing/etc",
  "confidence": 0.0-1.0,
  "filter_criteria": {{
    "from_pattern": "*.domain.com or specific@email.com",
    "subject_keywords": ["optional", "keywords"],
    "has_characteristics": ["list-unsubscribe", "automated-sender"]
  }},
  "reasoning": "why these patterns identify this category",
  "sample_gmail_query": "from:(*@domain.com) OR subject:(newsletter)"
}}

Focus on patterns that will work as Gmail filters without AI:
- Sender domains/patterns
- Subject line keywords
- Common email characteristics
"#, email_samples)
}

#[derive(Debug, Deserialize)]
struct PatternAnalysisResult {
    category: String,
    confidence: f32,
    filter_criteria: FilterCriteria,
    reasoning: String,
    sample_gmail_query: String,
}

#[derive(Debug, Deserialize)]
struct FilterCriteria {
    from_pattern: String,
    subject_keywords: Option<Vec<String>>,
    has_characteristics: Vec<String>,
}
```

**Acceptance Criteria:**
- LLM pattern analysis generates Gmail-compatible filter rules
- Generated filters work without ongoing LLM access
- Filters are deterministic and reproducible
- Falls back to rule-based if LLM fails
- Processing time optimised by analysing sender groups, not individual emails

#### FR-3.5: Claude Agents SDK Integration (Optional)
**Priority:** Could Have  
**Description:** Support Anthropic's Claude Agents SDK for advanced pattern analysis during initial filter generation.

**Challenge:** Claude Agents SDK is not available as a Rust crate, requiring alternative integration approaches.

**Integration Options:**

**Option 1: Python Bridge via PyO3 (Recommended)**
- Embed Python interpreter in Rust binary using PyO3
- Call Claude Agents SDK from Rust code
- Maintain type safety with Rust wrappers

```rust
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub struct ClaudeAgentBridge {
    py: Python<'static>,
    agent_module: PyObject,
}

impl ClaudeAgentBridge {
    pub fn new() -> Result<Self, PyErr> {
        Python::with_gil(|py| {
            let agent_module = py.import("claude_agents")?;
            Ok(Self {
                py,
                agent_module: agent_module.into(),
            })
        })
    }
    
    pub async fn analyse_email_patterns(
        &self,
        emails: Vec<MessageMetadata>
    ) -> Result<Vec<PatternAnalysisResult>, Error> {
        // Call Python SDK from Rust
        Python::with_gil(|py| {
            let agent = self.agent_module.getattr(py, "Agent")?;
            let result = agent.call_method1(py, "analyse", (emails,))?;
            // Convert Python result to Rust types
            Ok(result.extract(py)?)
        })
    }
}
```

**Pros:**
- Direct access to Claude Agents SDK features
- Type-safe Rust interface
- Single binary deployment possible
**Cons:**
- Requires Python runtime bundled with binary
- Increased binary size (~50MB)
- Complexity of managing Python interpreter

**Option 2: Microservice Architecture**
- Separate Python service running Claude Agents SDK
- Rust binary communicates via HTTP/gRPC
- Deploy Python service as sidecar or separate container

```rust
pub struct ClaudeAgentClient {
    client: reqwest::Client,
    base_url: String,
}

impl ClaudeAgentClient {
    pub async fn analyse_patterns(
        &self,
        emails: Vec<MessageMetadata>
    ) -> Result<Vec<PatternAnalysisResult>, Error> {
        let response = self.client
            .post(format!("{}/analyse", self.base_url))
            .json(&emails)
            .send()
            .await?;
            
        Ok(response.json().await?)
    }
}
```

**Pros:**
- Clean separation of concerns
- Independent scaling
- Python service can be reused
**Cons:**
- Deployment complexity (two services)
- Network latency
- Additional infrastructure requirements

**Option 3: CLI Wrapper**
- Package Rust binary with Python CLI tool
- Rust spawns Python process when Claude Agents needed
- Communicate via stdin/stdout with JSON

```rust
use tokio::process::Command;
use serde_json;

pub async fn call_claude_agent(
    emails: Vec<MessageMetadata>
) -> Result<Vec<PatternAnalysisResult>, Error> {
    let input = serde_json::to_string(&emails)?;
    
    let output = Command::new("python")
        .arg("-m")
        .arg("claude_agent_wrapper")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()
        .await?;
    
    let result: Vec<PatternAnalysisResult> = 
        serde_json::from_slice(&output.stdout)?;
    Ok(result)
}
```

**Pros:**
- Simplest integration
- No binary bloat
- Easy debugging
**Cons:**
- Requires Python installation on user system
- Process spawning overhead
- Less elegant user experience

**Option 4: Native Anthropic API (Without Agents SDK)**
- Use standard Anthropic Messages API from Rust
- Implement agent-like patterns in Rust
- Leverage structured outputs and tool use

```rust
use reqwest::Client;

pub struct AnthropicClient {
    client: Client,
    api_key: String,
}

impl AnthropicClient {
    pub async fn analyse_with_tools(
        &self,
        emails: Vec<MessageMetadata>
    ) -> Result<PatternAnalysisResult, Error> {
        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 4096,
                "tools": [/* Define analysis tools */],
                "messages": [/* Email analysis prompt */]
            }))
            .send()
            .await?;
            
        // Parse response with tool use
        Ok(parse_analysis_response(response).await?)
    }
}
```

**Pros:**
- Pure Rust implementation
- No Python dependency
- Most control over behaviour
**Cons:**
- Manual implementation of agent patterns
- Misses specialised Agents SDK features
- More development effort

**Recommended Approach:**

For this project, **Option 1 (PyO3 Bridge)** is recommended because:
1. Provides full Claude Agents SDK capabilities
2. Maintains Rust's type safety and performance
3. Can still produce a single distributable binary
4. Users don't need separate Python installation
5. Best balance of functionality vs. complexity

**Implementation Strategy:**
```toml
[dependencies]
pyo3 = { version = "0.20", features = ["auto-initialize"], optional = true }

[features]
claude-agents = ["pyo3"]
```

Enable with: `cargo build --features claude-agents`

**Acceptance Criteria:**
- Claude Agents SDK integration works seamlessly
- Pattern analysis quality exceeds direct API calls
- User can opt-in to Claude Agents feature
- Falls back to standard Anthropic API if SDK unavailable
- Clear documentation on setup and requirements

---

### 3.4 Label Management

#### FR-4.1: Label Structure Design
**Priority:** Must Have  
**Description:** Create hierarchical, non-redundant label structure.

**Label Hierarchy:**
```
AutoManaged/
├── Newsletters/
│   ├── Tech
│   ├── Business
│   └── Personal
├── Receipts/
│   ├── Shopping
│   └── Services
├── Notifications/
│   ├── Social
│   ├── Account
│   └── Service
├── Marketing/
│   └── [Company Names]
├── Shipping/
└── Financial/
```

**Requirements:**
- Use consistent prefix (e.g., "AutoManaged/") for all automated labels
- Create nested labels using "/" separator
- Limit nesting to 3 levels maximum
- Use title case for label names
- Deduplicate similar label names

**Acceptance Criteria:**
- Label hierarchy is logical and browsable
- No duplicate or near-duplicate labels created
- Labels follow consistent naming convention
- User can preview label structure before creation

#### FR-4.2: Label Creation
**Priority:** Must Have  
**Description:** Create labels in Gmail via API.

**Requirements:**
- Check if label exists before creation
- Create parent labels before child labels
- Set appropriate visibility: `labelShow`, `show`
- Handle API errors (rate limits, duplicates)
- Track all created labels for rollback capability

**API Call:**
```rust
use google_gmail1::{Gmail, api::Label};

async fn create_label(hub: &Gmail<impl yup_oauth2::GetToken>, 
                     label_name: &str) -> Result<Label, Error> {
    let label = Label {
        name: Some(label_name.to_string()),
        label_list_visibility: Some("labelShow".to_string()),
        message_list_visibility: Some("show".to_string()),
        ..Default::default()
    };
    
    let result = hub.users()
        .labels_create(label, "me")
        .doit()
        .await?;
    
    Ok(result.1)
}
```

**Acceptance Criteria:**
- All required labels created successfully
- Parent/child relationships maintained
- No duplicate labels created
- Label creation errors logged and handled

#### FR-4.3: Label Consolidation
**Priority:** Must Have  
**Description:** Prevent label proliferation through intelligent consolidation.

**Consolidation Rules:**
1. **Minimum Threshold:** Domains with <5 emails use generic category label
2. **Similar Names:** Detect and merge semantically similar labels
   - "GitHub Notifications" + "Github Alerts" → "GitHub"
3. **Subdomain Handling:** Consolidate subdomains
   - `news.company.com`, `marketing.company.com` → "Company"
4. **Known Services:** Use curated list of common services with standard names

**Process:**
1. Generate initial label list from classification
2. Apply consolidation rules
3. Present consolidated list to user for approval
4. Allow user to merge/split/rename labels
5. Create final approved label set

**Acceptance Criteria:**
- Initial label count reduced by 40-60% after consolidation
- User can review and modify consolidation decisions
- Consolidation logic is transparent and explainable

#### FR-4.4: Label Application
**Priority:** Must Have  
**Description:** Apply labels to historical messages.

**Requirements:**
- Use `messages.modify` API to add labels
- Process modifications concurrently with rate limiting
- Respect API rate limits (250 units/second)
- Track application progress
- Handle partial failures gracefully

**Process:**
1. Group messages by target label
2. Create concurrent stream of modify operations (40-50 concurrent)
3. Apply label to each message
4. Log successful and failed applications
5. Retry failed operations with exponential backoff

**Acceptance Criteria:**
- Labels applied to >99% of classified messages
- Progress indicator shows application status
- Failed applications logged with reason
- Idempotent: safe to re-run without duplicating labels

---

### 3.5 Filter Management

#### FR-5.1: Filter Generation
**Priority:** Must Have  
**Description:** Generate deterministic Gmail filters for automatic future categorisation without requiring AI/ML.

**Critical Design Principle:** All generated filters must work using Gmail's native filter syntax. No ongoing AI processing, external services, or custom code execution is required after initial setup.

**Filter Structure:**
```rust
use google_gmail1::api::{Filter, FilterCriteria, FilterAction};

let filter = Filter {
    criteria: Some(FilterCriteria {
        from: Some("sender@domain.com".to_string()),
        // or for domain-wide:
        // query: Some("from:(*@domain.com)".to_string()),
        ..Default::default()
    }),
    action: Some(FilterAction {
        add_label_ids: Some(vec!["LABEL_ID".to_string()]),
        remove_label_ids: Some(vec!["INBOX".to_string()]), // for auto-archive
        ..Default::default()
    }),
    ..Default::default()
};
```

**Filter Generation Strategy:**
1. **Group emails by sender domain** to identify patterns
2. **Extract deterministic characteristics:**
   - Sender email/domain patterns
   - Subject line keywords
   - Presence of unsubscribe headers
   - Common text patterns
3. **Generate Gmail query syntax** that captures these patterns
4. **Validate filters are deterministic** - same input always produces same result
5. **Test filters work without external dependencies**

**Requirements:**
- Create one filter per significant sender/domain pattern
- Use domain-wide patterns for newsletters and notifications
- Auto-archive non-actionable categories
- Keep important categories in inbox
- Limit filters to 500 (Gmail's practical limit)
- All filters must be Gmail-native (no external processing)

**Filter Priority:**
1. High-volume senders (>20 emails in period)
2. Clearly categorised domains
3. User-specified important filters

**Acceptance Criteria:**
- Filters created for top 80% of email volume
- Filters use domain patterns where appropriate
- Archive action applied to appropriate categories
- Filters can be previewed before creation
- **All filters work in Gmail without external dependencies**
- **Filters produce consistent, predictable results**

#### FR-5.2: Filter Creation
**Priority:** Must Have  
**Description:** Create filters in Gmail via API.

**Requirements:**
- Use `users.settings.filters.create` API
- Validate filter syntax before creation
- Check for conflicting existing filters
- Handle maximum filter limit (1,000)
- Track created filters for management

**Acceptance Criteria:**
- All generated filters created successfully
- No duplicate or conflicting filters
- Filter creation errors logged
- User can disable filter creation (label only mode)

#### FR-5.3: Filter Deduplication
**Priority:** Must Have  
**Description:** Prevent duplicate or overlapping filters.

**Requirements:**
- Retrieve existing filters via API
- Compare new filters against existing
- Detect overlapping criteria
- Prompt user to replace or skip duplicate filters
- Merge compatible filters where possible

**Deduplication Logic:**
- Exact match: Same criteria and action → Skip
- Subset match: New filter is subset of existing → Skip
- Superset match: New filter encompasses existing → Prompt to replace
- Conflict: Same criteria, different action → Prompt user

**Acceptance Criteria:**
- No duplicate filters created
- User prompted for conflicts
- Existing filters preserved unless explicitly replaced

#### FR-5.4: Retroactive Filter Application
**Priority:** Must Have  
**Description:** Apply filter logic to existing messages.

**Requirements:**
- Search for messages matching filter criteria
- Apply corresponding labels
- Remove from inbox if filter specifies archive
- Log all modifications
- Support dry-run mode to preview changes

**Process:**
1. For each created filter:
   - Build search query from criteria
   - Use `messages.list` to find matching messages
   - Apply label to all matches
   - Remove INBOX label if archive specified
2. Track total modifications
3. Report statistics to user

**Acceptance Criteria:**
- Filter actions applied to all matching historical messages
- Modification count matches expectations
- Dry-run mode shows changes without applying
- User can approve/reject retroactive application

---

### 3.6 User Interface

#### FR-6.1: Configuration Interface
**Priority:** Must Have  
**Description:** Allow user to configure system behaviour.

**Configuration Options:**
```toml
# config.toml
[scan]
period_days = 90
max_concurrent_requests = 40  # Stay under 250 units/sec rate limit

[classification]
mode = "rules"  # "rules", "ml", or "hybrid"
llm_provider = "openai"  # "openai", "anthropic", "anthropic-agents"
minimum_emails_for_label = 5

# Claude Agents SDK configuration (optional)
[classification.claude_agents]
enabled = false  # Requires building with --features claude-agents
use_advanced_analysis = true  # Use multi-step reasoning
max_iterations = 3  # For complex pattern detection

[labels]
prefix = "AutoManaged"
auto_archive_categories = [
    "newsletters",
    "notifications",
    "marketing"
]

[execution]
dry_run = false
```

**Acceptance Criteria:**
- Configuration stored in human-readable format
- Sensible defaults provided
- Invalid configuration detected with clear errors
- Configuration changes persist between runs

#### FR-6.2: Progress Reporting
**Priority:** Must Have  
**Description:** Provide real-time feedback during processing.

**Progress Indicators:**
1. Scanning phase: "Scanning emails... 1234/5678 (22%)"
2. Classification phase: "Classifying... 234/1234 (19%)"
3. Label creation phase: "Creating labels... 12/45 (27%)"
4. Filter creation phase: "Creating filters... 8/23 (35%)"
5. Application phase: "Applying labels... 567/1234 (46%)"

**Requirements:**
- Show current phase and progress percentage
- Display estimated time remaining
- Show current operation details
- Allow cancellation with graceful shutdown

**Acceptance Criteria:**
- Progress updates at least every 2 seconds
- Time estimates within 20% accuracy
- Clean display without flicker
- Cancellation preserves completed work

#### FR-6.3: Report Generation
**Priority:** Must Have  
**Description:** Generate summary report of changes made.

**Report Contents:**
```markdown
# Email Management Report
Generated: 2025-11-24 14:32:00

## Summary
- Emails scanned: 5,432
- Time period: 2025-08-24 to 2025-11-24
- Processing time: 12 minutes 34 seconds

## Classification Results
- Newsletters: 2,341 (43%)
- Notifications: 1,234 (23%)
- Receipts: 567 (10%)
- Marketing: 789 (15%)
- Other: 501 (9%)

## Labels Created
- Total labels: 23
- Top-level categories: 5
- Company-specific labels: 18

## Filters Created
- Total filters: 18
- Auto-archive filters: 14
- Inbox filters: 4

## Actions Taken
- Messages labelled: 5,432
- Messages archived: 4,123
- Messages kept in inbox: 1,309

## Top Senders
1. newsletters@company.com (543 emails) → Company Newsletter
2. notifications@github.com (432 emails) → GitHub
3. orders@amazon.com (234 emails) → Amazon
...
```

**Acceptance Criteria:**
- Report generated in Markdown and HTML formats
- Statistics accurate and complete
- Report saved to file automatically
- Can be shared or printed easily

#### FR-6.4: Review and Approval Interface
**Priority:** Should Have  
**Description:** Allow user to review and modify proposed changes before application.

**Review Screens:**
1. **Label Review:** Show proposed label structure with message counts
2. **Filter Review:** Show filter criteria and affected message count
3. **Sample Review:** Show sample messages for each category

**User Actions:**
- Approve all
- Approve selected
- Modify label names
- Merge categories
- Exclude specific senders
- Adjust archive/inbox decisions

**Acceptance Criteria:**
- All changes reviewable before application
- User can modify any decision
- Changes reflected in final execution
- Modifications saved for future runs

---

### 3.7 Error Handling & Recovery

#### FR-7.1: API Error Handling
**Priority:** Must Have  
**Description:** Gracefully handle Gmail API errors.

**Error Types:**
1. **Rate Limit (429):** Implement exponential backoff, retry
2. **Authentication (401):** Prompt re-authentication
3. **Quota Exceeded:** Log error, suggest scheduling for next day
4. **Network Errors:** Retry with timeout
5. **Invalid Requests (400):** Log details, skip problematic request

**Requirements:**
- All API calls wrapped in error handling
- Errors logged with full context
- Transient errors retried automatically
- Permanent errors reported to user
- Processing continues after recoverable errors

**Acceptance Criteria:**
- No crashes from API errors
- Transient errors retried successfully
- User informed of permanent errors
- Partial results preserved on fatal errors

#### FR-7.2: State Persistence
**Priority:** Should Have  
**Description:** Save progress to enable resumption after interruption.

**State Data:**
- Messages scanned
- Classifications completed
- Labels created
- Filters created
- Labels applied

**Requirements:**
- Save state to disk after each major phase
- Resume from last saved state on restart
- Detect incomplete runs
- Allow forced restart from beginning

**Acceptance Criteria:**
- Interrupted runs can resume without re-processing
- State file is human-readable (JSON)
- Corrupted state files detected and handled
- User can reset state manually

#### FR-7.3: Rollback Capability
**Priority:** Should Have  
**Description:** Provide mechanism to undo changes.

**Rollback Actions:**
- Remove created labels
- Delete created filters
- Remove applied labels from messages
- Restore messages to inbox

**Requirements:**
- Track all created resources in rollback log
- Provide rollback command/option
- Confirm before executing rollback
- Support partial rollback (e.g., filters only)

**Acceptance Criteria:**
- All changes can be undone
- Rollback completes without errors
- Account restored to pre-execution state
- Rollback log persists until explicitly cleared

---

## 4. Non-Functional Requirements

### 4.1 Performance

**PR-1: Processing Speed**
- Process at least 100 emails per minute
- Complete typical inbox (5,000 emails) within 30 minutes
- Operate within API rate limits without throttling
- Leverage Rust's async runtime (Tokio) for concurrent API requests

**PR-2: Resource Usage**
- Memory usage <200MB for typical operation (Rust's efficient memory model)
- Disk space <100MB for cache and logs
- Network bandwidth <50MB for 10,000 emails
- Near-zero runtime overhead from async operations

**PR-3: API Efficiency**
- Use concurrent individual requests with rate limiting (NOT native batching)
- Cache API responses to avoid redundant calls
- Minimise quota unit consumption
- Parallel processing with bounded concurrency (40-50 concurrent operations)

### 4.2 Reliability

**RE-1: Data Integrity**
- No loss of email data or metadata
- Accurate classification >90% of time
- Consistent label/filter application
- Type safety guaranteed by Rust's compiler

**RE-2: Fault Tolerance**
- Recover from network interruptions
- Resume after application crashes
- Handle partial API failures
- Graceful error handling with `Result` and `anyhow`

**RE-3: Idempotency**
- Safe to re-run without duplicating changes
- Detect and skip already-processed messages
- Prevent duplicate label/filter creation

### 4.3 Usability

**US-1: Ease of Use**
- Simple installation via Cargo (<2 minutes)
- Clear documentation and examples
- Intuitive configuration options
- Single binary deployment (no dependencies)

**US-2: Feedback**
- Real-time progress updates via `indicatif` progress bars
- Clear error messages with context
- Actionable recommendations

**US-3: Control**
- User can review before changes applied
- User can modify automated decisions
- User can rollback changes

### 4.4 Security

**SE-1: Authentication**
- OAuth2 for secure API access via `yup-oauth2`
- No storage of passwords
- Credentials stored with secure file permissions

**SE-2: Data Privacy**
- No email content sent to external services (unless user enables ML mode)
- Local processing by default
- Explicit consent for LLM usage

**SE-3: Permissions**
- Request only necessary OAuth scopes
- Explain why each permission is needed
- Support read-only mode for scanning only

### 4.5 Maintainability

**MA-1: Code Quality**
- Modular architecture with clear separation of concerns
- Comprehensive error handling with `thiserror` and `anyhow`
- Unit test coverage >80%
- Type safety and ownership guaranteed by Rust compiler
- Documentation with `rustdoc`

**MA-2: Logging**
- Structured logging with `tracing` crate
- Configurable log verbosity with `tracing-subscriber`
- Log rotation for long-running processes
- Async logging to avoid I/O blocking

**MA-3: Extensibility**
- Trait-based architecture for classification rules
- Configurable label hierarchy via TOML
- Support custom LLM providers via trait implementations
- Plugin system for custom classifiers

---

## 5. Data Model

### 5.1 Core Entities

#### Message
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub sender_email: String,
    pub sender_domain: String,
    pub sender_name: String,
    pub subject: String,
    pub recipients: Vec<String>,
    pub date_received: DateTime<Utc>,
    pub existing_labels: Vec<String>,
    pub body_preview: Option<String>,
    pub has_unsubscribe: bool,
    pub is_reply: bool,
    pub is_forward: bool,
}
```

#### Classification
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub message_id: String,
    pub primary_category: String,
    pub secondary_categories: Vec<String>,
    pub confidence: f32,
    pub suggested_label: String,
    pub should_archive: bool,
    pub classification_method: ClassificationMethod,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClassificationMethod {
    Rule,
    Ml,
    Manual,
}
```

#### Label
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: Option<String>,  // Gmail label ID after creation
    pub name: String,  // Full hierarchical name
    pub parent: Option<String>,
    pub message_count: usize,
    pub created_by_system: bool,
    pub visibility: LabelVisibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LabelVisibility {
    LabelShow,
    LabelShowIfUnread,
    LabelHide,
}
```

#### Filter
```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub id: Option<String>,  // Gmail filter ID after creation
    pub criteria: HashMap<String, serde_json::Value>,
    pub action: HashMap<String, serde_json::Value>,
    pub estimated_matches: usize,
    pub created_by_system: bool,
}
```

### 5.2 State Management

#### ProcessingState
```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingState {
    pub run_id: String,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub phase: ProcessingPhase,
    pub messages_scanned: usize,
    pub messages_classified: usize,
    pub labels_created: Vec<String>,
    pub filters_created: Vec<String>,
    pub messages_modified: usize,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingPhase {
    Scanning,
    Classifying,
    CreatingLabels,
    CreatingFilters,
    ApplyingLabels,
    Complete,
}
```

---

## 6. API Integration Specifications

### 6.1 Gmail API Endpoints

#### Messages
- `GET /gmail/v1/users/me/messages` - List messages
- `GET /gmail/v1/users/me/messages/{id}` - Get message
- `POST /gmail/v1/users/me/messages/batchModify` - Batch modify labels (may be used for applying labels to multiple messages)

#### Labels
- `GET /gmail/v1/users/me/labels` - List labels
- `POST /gmail/v1/users/me/labels` - Create label
- `GET /gmail/v1/users/me/labels/{id}` - Get label
- `DELETE /gmail/v1/users/me/labels/{id}` - Delete label

#### Filters
- `GET /gmail/v1/users/me/settings/filters` - List filters
- `POST /gmail/v1/users/me/settings/filters` - Create filter
- `GET /gmail/v1/users/me/settings/filters/{id}` - Get filter
- `DELETE /gmail/v1/users/me/settings/filters/{id}` - Delete filter

### 6.2 Quota Management

**Quota Units:**
- `messages.list`: 5 units
- `messages.get`: 5 units
- `messages.modify`: 5 units
- `labels.create`: 5 units
- `filters.create`: 5 units

**Limits:**
- Per user per second: 250 units
- Theoretical max: 50 operations/second
- Recommended: 25 operations/second (conservative)

**Concurrency Limits:**
- Maximum 40-50 concurrent requests recommended (to stay under 250 units/sec)
- Use semaphores or buffered streams to enforce limits

**Note:** While Gmail API supports native batch requests (100 per batch), we use concurrent individual requests because quota savings don't exist and the concurrent approach provides better error handling.

### 6.3 Rate Limiting Strategy

```rust
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct RateLimiter {
    units_per_second: u32,
    max_units_per_100s: u32,
    current_second_units: Arc<Mutex<u32>>,
    current_100s_units: Arc<Mutex<u32>>,
    last_second_reset: Arc<Mutex<Instant>>,
    last_100s_reset: Arc<Mutex<Instant>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            units_per_second: 250,
            max_units_per_100s: 25_000,
            current_second_units: Arc::new(Mutex::new(0)),
            current_100s_units: Arc::new(Mutex::new(0)),
            last_second_reset: Arc::new(Mutex::new(Instant::now())),
            last_100s_reset: Arc::new(Mutex::new(Instant::now())),
        }
    }
    
    pub async fn acquire(&self, units: u32) -> Result<(), RateLimitError> {
        self.check_and_reset().await;
        
        let mut second_units = self.current_second_units.lock().unwrap();
        let mut hundred_s_units = self.current_100s_units.lock().unwrap();
        
        if *second_units + units > self.units_per_second {
            return Err(RateLimitError::PerSecondExceeded);
        }
        if *hundred_s_units + units > self.max_units_per_100s {
            return Err(RateLimitError::Per100sExceeded);
        }
        
        *second_units += units;
        *hundred_s_units += units;
        Ok(())
    }
    
    async fn check_and_reset(&self) {
        let now = Instant::now();
        
        // Reset per-second counter
        let mut last_reset = self.last_second_reset.lock().unwrap();
        if now.duration_since(*last_reset) >= Duration::from_secs(1) {
            *self.current_second_units.lock().unwrap() = 0;
            *last_reset = now;
        }
        
        // Reset per-100s counter
        let mut last_100s = self.last_100s_reset.lock().unwrap();
        if now.duration_since(*last_100s) >= Duration::from_secs(100) {
            *self.current_100s_units.lock().unwrap() = 0;
            *last_100s = now;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Per-second rate limit exceeded")]
    PerSecondExceeded,
    #[error("Per-100-second rate limit exceeded")]
    Per100sExceeded,
}
```

---

## 7. Classification Rules Specification

### 7.1 Sender Pattern Rules

```rust
use std::collections::HashMap;
use once_cell::sync::Lazy;

pub static AUTOMATED_PATTERNS: Lazy<HashMap<&str, Vec<&str>>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("noreply", vec!["noreply@", "no-reply@", "donotreply@"]);
    m.insert("notifications", vec!["notifications@", "notify@", "alerts@"]);
    m.insert("marketing", vec!["marketing@", "promo@", "offers@", "deals@"]);
    m.insert("newsletter", vec!["newsletter@", "news@", "digest@"]);
    m.insert("info", vec!["info@", "hello@", "contact@"]);
    m.insert("support", vec!["support@", "help@", "service@"]);
    m
});

pub static COMMERCIAL_DOMAINS: &[&str] = &[
    "sendgrid.net",
    "mailchimp.com",
    "constantcontact.com",
    "postmarkapp.com",
    "amazonses.com",
    "mailgun.org",
];
```

### 7.2 Subject Pattern Rules

```rust
use regex::Regex;
use once_cell::sync::Lazy;

pub static NEWSLETTER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(newsletter|weekly|monthly|digest|roundup)\b|(edition|issue)\s+\d+")
        .unwrap()
});

pub static RECEIPT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(receipt|invoice|order|payment|purchase)\b|order\s+#?\d+|confirmation\s+#?\d+")
        .unwrap()
});

pub static NOTIFICATION_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(notification|alert|reminder|update)\b|you\s+have\s+\d+\s+new")
        .unwrap()
});

pub static SHIPPING_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(shipped|tracking|delivery|dispatched|package)\b|tracking\s+number")
        .unwrap()
});

#[derive(Debug)]
pub struct SubjectPatterns {
    pub newsletter: &'static Lazy<Regex>,
    pub receipt: &'static Lazy<Regex>,
    pub notification: &'static Lazy<Regex>,
    pub shipping: &'static Lazy<Regex>,
}

impl SubjectPatterns {
    pub fn new() -> Self {
        Self {
            newsletter: &NEWSLETTER_PATTERN,
            receipt: &RECEIPT_PATTERN,
            notification: &NOTIFICATION_PATTERN,
            shipping: &SHIPPING_PATTERN,
        }
    }
}
```

### 7.3 Known Service Domains

```rust
use std::collections::HashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub name: String,
    pub category: String,
    pub archive: bool,
}

pub static KNOWN_SERVICES: Lazy<HashMap<&str, ServiceInfo>> = Lazy::new(|| {
    let mut m = HashMap::new();
    
    m.insert("github.com", ServiceInfo {
        name: "GitHub".to_string(),
        category: "notifications".to_string(),
        archive: true,
    });
    
    m.insert("linkedin.com", ServiceInfo {
        name: "LinkedIn".to_string(),
        category: "social".to_string(),
        archive: true,
    });
    
    m.insert("amazon.com", ServiceInfo {
        name: "Amazon".to_string(),
        category: "receipts".to_string(),
        archive: true,
    });
    
    m.insert("netflix.com", ServiceInfo {
        name: "Netflix".to_string(),
        category: "notifications".to_string(),
        archive: true,
    });
    
    // ... extensive list continues
    m
});
```

### 7.4 Priority Scoring

```rust
pub fn calculate_priority_score(message: &Message) -> f32 {
    let mut score = 0.5; // baseline
    
    // Direct personal email (not automated)
    if !is_automated_sender(&message.sender_email) {
        score += 0.3;
    }
    
    // Part of active conversation
    if is_active_thread(&message.thread_id) {
        score += 0.2;
    }
    
    // Contains personal indicators
    if contains_personal_indicators(message) {
        score += 0.2;
    }
    
    // From unknown sender
    if is_first_time_sender(&message.sender_email) {
        score += 0.1;
    }
    
    // Penalties
    if message.has_unsubscribe {
        score -= 0.3;
    }
    
    if is_promotional(message) {
        score -= 0.2;
    }
    
    score.clamp(0.0, 1.0)
}

fn is_automated_sender(email: &str) -> bool {
    AUTOMATED_PATTERNS.values()
        .flat_map(|patterns| patterns.iter())
        .any(|pattern| email.contains(pattern))
}

fn is_active_thread(thread_id: &str) -> bool {
    // Implementation to check if thread has recent activity
    // This would query recent messages in the thread
    todo!()
}

fn contains_personal_indicators(message: &Message) -> bool {
    // Check for personal salutations, direct addressing, etc.
    todo!()
}

fn is_first_time_sender(email: &str) -> bool {
    // Check if this sender has been seen before
    todo!()
}

fn is_promotional(message: &Message) -> bool {
    // Check for promotional indicators
    AUTOMATED_PATTERNS.get("marketing")
        .map(|patterns| patterns.iter().any(|p| message.sender_email.contains(p)))
        .unwrap_or(false)
}
```

---

## 8. Testing Requirements

### 8.1 Unit Tests

**Modules to Test:**
- Email scanning and pagination logic
- Classification rules and pattern matching
- Label name normalisation and deduplication
- Filter criteria generation
- Quota management and rate limiting

**Coverage Target:** >80%

**Testing Framework:**
```toml
[dev-dependencies]
tokio-test = "0.4"
mockito = "1.2"
proptest = "1.4"
```

**Example Test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_calculate_priority_score() {
        let message = Message {
            sender_email: "person@company.com".to_string(),
            has_unsubscribe: false,
            // ... other fields
        };
        
        let score = calculate_priority_score(&message);
        assert!(score > 0.5);
    }
    
    #[tokio::test]
    async fn test_concurrent_message_retrieval() {
        // Test async concurrent operations
    }
}
```

### 8.2 Integration Tests

**Test Scenarios:**
1. End-to-end flow with test Gmail account
2. Authentication flow and token refresh
3. Concurrent API operations with rate limiting
4. Error recovery and retry logic
5. State persistence and resumption

### 8.3 User Acceptance Tests

**Test Cases:**
1. Scan 1,000 emails, verify all retrieved
2. Classify sample emails, validate accuracy
3. Review generated labels for sensibility
4. Verify filters work on new emails
5. Confirm rollback removes all changes

---

## 9. Deployment & Operations

### 9.1 Installation

```bash
# Clone repository
git clone https://github.com/user/gmail-automation.git
cd gmail-automation

# Build release version (optimised)
cargo build --release

# Configure
cp config.example.toml config.toml
# Edit config.toml with preferences

# Authenticate
./target/release/gmail-automation auth

# Run
./target/release/gmail-automation run
```

**Alternative: Install via Cargo**
```bash
cargo install gmail-automation
gmail-automation auth
gmail-automation run
```

### 9.2 Configuration Files

```
gmail-automation/
├── Cargo.toml           # Rust project manifest
├── Cargo.lock          # Dependency lock file
├── config.toml         # User configuration
├── credentials.json    # OAuth client credentials
├── src/                # Source code
│   ├── main.rs
│   ├── lib.rs
│   ├── auth.rs
│   ├── scanner.rs
│   ├── classifier.rs
│   ├── label_manager.rs
│   └── filter_manager.rs
├── .gmail-automation/  # User data directory
│   ├── token.json     # User OAuth token (generated)
│   ├── state.json     # Processing state (generated)
│   └── rollback.json  # Rollback log (generated)
└── logs/              # Application logs
```

### 9.3 Operational Modes

**Mode 1: Dry Run**
```bash
gmail-automation run --dry-run
```
- Scan and classify emails
- Generate report
- No changes to Gmail account

**Mode 2: Labels Only**
```bash
gmail-automation run --labels-only
```
- Create labels and apply to messages
- Do not create filters

**Mode 3: Full Automation**
```bash
gmail-automation run
```
- Complete end-to-end processing

**Mode 4: Review Mode**
```bash
gmail-automation run --interactive
```
- Stop before each major action for user approval

### 9.4 Monitoring & Maintenance

**Logging:**
- Application log: `logs/gmail_automation.log`
- Error log: `logs/errors.log`
- API log: `logs/api_calls.log`
- Uses `tracing` and `tracing-subscriber` for structured logging

**Metrics to Track:**
- API quota usage
- Classification accuracy
- Processing time
- Error rates

**Maintenance Tasks:**
- Review classification rules quarterly
- Update known services list
- Prune old logs (>30 days)
- Update dependencies

---

## 10. Future Enhancements

### 10.1 Phase 2 Features

**P2-1: Incremental Processing**
- Scheduled daily runs for new emails
- Delta processing mode
- Automatic maintenance of labels/filters

**P2-2: Machine Learning Improvements**
- Fine-tune classification with user feedback
- Learn from user corrections
- Personalised classification models

**P2-3: Advanced Analytics**
- Email volume trends over time
- Sender behaviour analysis
- Inbox health metrics
- Subscription cost analysis (for paid newsletters)

**P2-4: Multi-Account Support**
- Manage multiple Gmail accounts
- Unified dashboard
- Sync settings across accounts

### 10.2 Phase 3 Features

**P3-1: Smart Actions**
- Automatic unsubscribe from unwanted senders
- Bulk deletion of old promotional emails
- Priority inbox segregation

**P3-2: Integration**
- Export to task management tools
- Calendar integration for event emails
- CRM integration for contact emails

**P3-3: Web Interface**
- Browser-based UI for configuration
- Visual label hierarchy editor
- Real-time processing dashboard

---

## 11. Success Metrics

### 11.1 Quantitative Metrics

- **Inbox Reduction:** >80% reduction in inbox count
- **Classification Accuracy:** >90% correct categorisation during initial analysis
- **Filter Quality:** Generated filters correctly categorise >95% of future emails
- **Processing Efficiency:** Complete 5,000 emails in <30 minutes
- **API Efficiency:** Operate within 50% of quota limits
- **Label Quality:** <50 labels created for typical users
- **Filter Determinism:** 100% of filters work without external dependencies
- **User Satisfaction:** >4/5 rating from test users

### 11.2 Qualitative Metrics

- Users report improved email findability
- Users feel less overwhelmed by inbox
- Users trust automated categorisation
- Users spend less time on email management
- Users can easily locate important emails
- **Filters continue to work correctly without ongoing maintenance**
- **New emails are automatically categorised correctly**

---

## 12. Risk Assessment

### 12.1 Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| API quota exhaustion | Medium | High | Implement conservative rate limiting, provide scheduling options |
| Misclassification | Medium | Medium | Provide review mode, easy manual correction |
| Data loss | Low | Critical | Never delete emails, all actions reversible |
| Authentication issues | Low | High | Clear error messages, simple re-auth flow |
| Network failures | Medium | Low | Implement retry logic, save state frequently |

### 12.2 User Experience Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Label proliferation | High | Medium | Strong consolidation rules, user review |
| Unexpected archiving | Medium | High | Conservative archive rules, dry run mode |
| Overwhelming configuration | Medium | Low | Sensible defaults, progressive disclosure |
| Technical complexity | Medium | Medium | Clear documentation, simple CLI |

---

## 13. Glossary

- **Concurrent Requests:** Multiple API operations executed in parallel with rate limiting
- **Domain Clustering:** Grouping emails by sender domain for organised labelling
- **Filter:** Gmail rule that automatically processes incoming emails
- **Label:** Gmail's equivalent of folders; emails can have multiple labels
- **OAuth2:** Authentication protocol for secure API access
- **Quota Units:** Abstract measure of Gmail API resource usage
- **Rate Limiting:** Throttling API requests to stay within usage limits
- **Retroactive Application:** Applying filter logic to existing emails

---

## 14. Key Dependencies (Cargo.toml)

```toml
[package]
name = "gmail-automation"
version = "0.1.0"
edition = "2021"
rust-version = "1.70"

[dependencies]
# Gmail API
google-gmail1 = "5.0"
yup-oauth2 = "8.3"

# Async runtime
tokio = { version = "1.35", features = ["full"] }
futures = "0.3"

# HTTP client
reqwest = { version = "0.11", features = ["json"] }
hyper = "0.14"

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# Date/time
chrono = { version = "0.4", features = ["serde"] }

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# CLI
clap = { version = "4.4", features = ["derive"] }
indicatif = "0.17"
console = "0.15"

# TUI (optional)
ratatui = { version = "0.25", optional = true }
crossterm = { version = "0.27", optional = true }

# Regex
regex = "1.10"
once_cell = "1.19"

# Database (optional)
rusqlite = { version = "0.30", features = ["bundled"], optional = true }

# ML providers (optional)
async-openai = { version = "0.17", optional = true }

# Python bridge for Claude Agents SDK (optional)
pyo3 = { version = "0.20", features = ["auto-initialize"], optional = true }

[dev-dependencies]
tokio-test = "0.4"
mockito = "1.2"
proptest = "1.4"

[features]
default = ["cli"]
cli = []
tui = ["ratatui", "crossterm"]
ml = ["async-openai"]
cache = ["rusqlite"]
claude-agents = ["pyo3", "ml"]  # Requires Python runtime

[build-dependencies]
# For claude-agents feature: embed Python scripts
pyo3-build-config = { version = "0.20", optional = true }
```

**Claude Agents Feature:**
When built with `--features claude-agents`, the binary includes:
- Python interpreter via PyO3
- Bridge to Claude Agents SDK
- Automatic fallback to standard API if SDK unavailable

Build with: `cargo build --release --features claude-agents`

---

## 15. References

- Gmail API Documentation: https://developers.google.com/gmail/api
- Gmail API Usage Limits: https://developers.google.com/gmail/api/reference/quota
- OAuth 2.0 Documentation: https://developers.google.com/identity/protocols/oauth2
- Gmail Filter Documentation: https://developers.google.com/gmail/api/guides/filter_settings
- `google-gmail1` Rust Crate: https://docs.rs/google-gmail1/
- `yup-oauth2` Documentation: https://docs.rs/yup-oauth2/
- Tokio Async Runtime: https://tokio.rs/
- The Rust Programming Language: https://doc.rust-lang.org/book/

---

## 16. Document Change History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2025-11-24 | Victor Bajanov | Initial draft |
| 1.1 | 2025-11-24 | Victor Bajanov | Converted from Python to Rust implementation |

---

**End of Functional Design Specification**
