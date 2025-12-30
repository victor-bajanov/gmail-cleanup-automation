// Authentication types
export interface AuthStatus {
  authenticated: boolean;
  email: string | null;
  credentials_path: string;
  credentials_exist: boolean;
  token_exists: boolean;
}

// Scan types
export interface ScanOptions {
  period_days: number;
  min_cluster_size: number;
  query?: string;
}

export interface ScanResult {
  total_messages: number;
  fetched_messages: number;
  cluster_count: number;
  unique_domains: number;
}

export interface ScanProgress {
  phase: 'listing' | 'fetching' | 'classifying' | 'clustering' | 'complete';
  current: number;
  total: number;
  message?: string;
}

export interface DomainStat {
  domain: string;
  count: number;
}

// Message types
export interface MessageMetadata {
  id: string;
  thread_id: string;
  sender_email: string;
  sender_domain: string;
  sender_name: string;
  subject: string;
  recipients: string[];
  date_received: string;
  labels: string[];
  has_unsubscribe: boolean;
  is_automated: boolean;
}

// Cluster types
export interface ClusterView {
  index: number;
  sender_pattern: string;
  email_count: number;
  suggested_label: string;
  should_archive: boolean;
  sample_subjects: string[];
  sample_senders: string[];
  has_existing_filter: boolean;
  existing_filter_label?: string;
  decided: boolean;
  decision?: string;
}

export interface DecisionInput {
  cluster_index: number;
  action: string;
  custom_label?: string;
  archive?: boolean;
}

export interface ReviewSummary {
  total: number;
  accepted: number;
  rejected: number;
  skipped: number;
  deleted: number;
  excluded: number;
  remaining: number;
}

export interface ClusterDecision {
  cluster_index: number;
  action: DecisionAction;
  should_archive: boolean;
  target_label: string;
}

export type DecisionAction =
  | 'Accept'
  | 'Reject'
  | 'Skip'
  | 'Delete'
  | 'ExcludeForever'
  | { CustomLabel: string };

// Filter types
export interface FilterView {
  id?: string;
  name: string;
  query: string;
  label: string;
  archive: boolean;
  estimated_matches: number;
  is_proposed: boolean;
  change_type: 'unchanged' | 'new' | 'modified' | 'deleted';
}

export interface FilterComparison {
  existing: FilterView[];
  proposed: FilterView[];
  summary: FilterChangeSummary;
}

export interface FilterChangeSummary {
  unchanged: number;
  new: number;
  modified: number;
  deleted: number;
  total_affected: number;
}

export interface ApplyFiltersResult {
  success: boolean;
  created: number;
  failed: number;
  dry_run: boolean;
  errors: string[];
}

export interface FilterProgress {
  operation: 'fetching_existing' | 'analyzing_overlaps' | 'creating' | 'applying_retroactive' | 'deleting' | 'complete';
  current: number;
  total: number;
  filter_name?: string;
}

// Analysis types
export interface ConflictView {
  filter_a_id: string;
  filter_b_id: string;
  filter_a_name: string;
  filter_b_name: string;
  filter_a_query: string;
  filter_b_query: string;
  filter_a_label: string;
  filter_b_label: string;
  conflict_type: string;
  severity: 'Info' | 'Warning' | 'Error';
  description: string;
  suggestions: string[];
}

export interface AnalysisView {
  total_filters: number;
  conflicts: ConflictView[];
  error_count: number;
  warning_count: number;
  info_count: number;
  summary: string;
}

export interface FilterCoverage {
  filter_name: string;
  filter_query: string;
  email_count: number;
  percentage: number;
  is_existing: boolean;
}

export interface CoverageAnalysis {
  total_emails: number;
  covered_emails: number;
  uncovered_emails: number;
  coverage_percentage: number;
  filter_coverage: FilterCoverage[];
  top_uncovered_domains: DomainCount[];
}

export interface DomainCount {
  domain: string;
  count: number;
}

export interface OverlapMatrixEntry {
  filter_a_index: number;
  filter_b_index: number;
  filter_a_name: string;
  filter_b_name: string;
  relation: string;
  overlap_count: number;
}

export interface OverlapMatrixResult {
  filter_names: string[];
  matrix: OverlapMatrixEntry[];
}

export interface UncoveredEmail {
  id: string;
  sender: string;
  domain: string;
  subject: string;
  date: string;
}

export interface HiddenFilterInfo {
  id: string;
  name: string;
  query: string;
  label: string;
}

// Event types
export interface ErrorEvent {
  code: string;
  message: string;
  recoverable: boolean;
}

export interface ClusterEvent {
  event_type: 'selected' | 'decision_made' | 'decision_undone' | 'all_reviewed';
  cluster_index: number;
  data?: unknown;
}

// Settings types
export interface AppSettings {
  scan_period_days: number;
  min_cluster_size: number;
  label_prefix: string;
  default_archive: boolean;
  theme: 'light' | 'dark' | 'system';
  show_shortcuts: boolean;
  sound_enabled: boolean;
  auto_advance: boolean;
}

export interface WindowState {
  width: number;
  height: number;
  x?: number;
  y?: number;
  maximized: boolean;
}

// Config.toml settings (separate from GUI AppSettings)
export interface ConfigSettings {
  scan_period_days: number;
  max_concurrent_requests: number;
  classification_mode: string;
  llm_provider: string;
  minimum_emails_for_label: number;
  label_prefix: string;
  auto_archive_categories: string[];
  dry_run: boolean;
  circuit_breaker_enabled: boolean;
  failure_threshold: number;
  reset_timeout_secs: number;
}

// App state
export type AppView = 'auth' | 'scan' | 'review' | 'filters' | 'coverage' | 'settings';
