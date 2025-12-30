/**
 * Type-safe Tauri command wrappers
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  AuthStatus,
  ScanOptions,
  ScanResult,
  DomainStat,
  MessageMetadata,
  ClusterView,
  DecisionInput,
  ReviewSummary,
  ClusterDecision,
  FilterView,
  FilterComparison,
  ApplyFiltersResult,
  AnalysisView,
  CoverageAnalysis,
  OverlapMatrixResult,
  UncoveredEmail,
  HiddenFilterInfo,
  AppSettings,
  WindowState,
  ConfigSettings,
} from '../types';

// ============ Authentication Commands ============

export async function checkAuthStatus(): Promise<AuthStatus> {
  return invoke<AuthStatus>('check_auth_status');
}

export async function setCredentialsPath(path: string): Promise<boolean> {
  return invoke<boolean>('set_credentials_path', { path });
}

export async function authenticate(): Promise<AuthStatus> {
  return invoke<AuthStatus>('authenticate');
}

export async function logout(): Promise<boolean> {
  return invoke<boolean>('logout');
}

export async function initializeClient(): Promise<boolean> {
  return invoke<boolean>('initialize_client');
}

// ============ Scan Commands ============

export async function scanEmails(options: ScanOptions): Promise<ScanResult> {
  return invoke<ScanResult>('scan_emails', { options });
}

export async function getMessages(): Promise<MessageMetadata[]> {
  return invoke<MessageMetadata[]>('get_messages');
}

export async function getDomainStats(): Promise<DomainStat[]> {
  return invoke<DomainStat[]>('get_domain_stats');
}

export async function classifyMessages(): Promise<number> {
  return invoke<number>('classify_messages');
}

export async function clearScanData(): Promise<void> {
  return invoke<void>('clear_scan_data');
}

// ============ Cluster Commands ============

export async function getClusters(minEmails?: number): Promise<ClusterView[]> {
  return invoke<ClusterView[]>('get_clusters', { minEmails });
}

export async function getCluster(index: number): Promise<ClusterView> {
  return invoke<ClusterView>('get_cluster', { index });
}

export async function submitClusterDecision(input: DecisionInput): Promise<boolean> {
  return invoke<boolean>('submit_cluster_decision', { input });
}

export async function undoLastDecision(): Promise<number | null> {
  return invoke<number | null>('undo_last_decision');
}

export async function getReviewSummary(): Promise<ReviewSummary> {
  return invoke<ReviewSummary>('get_review_summary');
}

export async function getDecisions(): Promise<ClusterDecision[]> {
  return invoke<ClusterDecision[]>('get_decisions');
}

export async function clearDecisions(): Promise<void> {
  return invoke<void>('clear_decisions');
}

export async function getNextUndecidedCluster(): Promise<number | null> {
  return invoke<number | null>('get_next_undecided_cluster');
}

export async function skipAllExisting(): Promise<number> {
  return invoke<number>('skip_all_existing');
}

// ============ Filter Commands ============

export async function getExistingFilters(): Promise<FilterView[]> {
  return invoke<FilterView[]>('get_existing_filters');
}

export async function generateProposedFilters(): Promise<FilterView[]> {
  return invoke<FilterView[]>('generate_proposed_filters');
}

export async function compareFilters(): Promise<FilterComparison> {
  return invoke<FilterComparison>('compare_filters');
}

export async function applyFilters(dryRun: boolean): Promise<ApplyFiltersResult> {
  return invoke<ApplyFiltersResult>('apply_filters', { dryRun });
}

export async function deleteFilter(filterId: string): Promise<boolean> {
  return invoke<boolean>('delete_filter', { filterId });
}

// ============ Analysis Commands ============

export async function analyzeFilterOverlaps(includeInfo?: boolean, autoManagedOnly?: boolean): Promise<AnalysisView> {
  return invoke<AnalysisView>('analyze_filter_overlaps', { includeInfo, autoManagedOnly });
}

export async function hideFilter(
  filterId: string,
  filterName: string,
  filterQuery: string,
  filterLabel: string
): Promise<boolean> {
  return invoke<boolean>('hide_filter', { filterId, filterName, filterQuery, filterLabel });
}

export async function unhideFilter(filterId: string): Promise<boolean> {
  return invoke<boolean>('unhide_filter', { filterId });
}

export async function getHiddenFilters(): Promise<HiddenFilterInfo[]> {
  return invoke<HiddenFilterInfo[]>('get_hidden_filters');
}

export async function clearHiddenFilters(): Promise<boolean> {
  return invoke<boolean>('clear_hidden_filters');
}

export async function analyzeCoverage(): Promise<CoverageAnalysis> {
  return invoke<CoverageAnalysis>('analyze_coverage');
}

export async function getOverlapMatrix(): Promise<OverlapMatrixResult> {
  return invoke<OverlapMatrixResult>('get_overlap_matrix');
}

export async function getUncoveredEmails(limit?: number): Promise<UncoveredEmail[]> {
  return invoke<UncoveredEmail[]>('get_uncovered_emails', { limit });
}

// ============ Settings Commands ============

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings');
}

export async function saveSettings(settings: AppSettings): Promise<boolean> {
  return invoke<boolean>('save_settings', { settings });
}

export async function resetSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('reset_settings');
}

export async function getWindowState(): Promise<WindowState> {
  return invoke<WindowState>('get_window_state');
}

export async function saveWindowState(state: WindowState): Promise<boolean> {
  return invoke<boolean>('save_window_state', { state });
}

// ============ Config Settings Commands (config.toml) ============

export async function getConfigSettings(): Promise<ConfigSettings> {
  return invoke<ConfigSettings>('get_config_settings');
}

export async function saveConfigSettings(settings: ConfigSettings): Promise<boolean> {
  return invoke<boolean>('save_config_settings', { settings });
}

export async function resetConfigSettings(): Promise<ConfigSettings> {
  return invoke<ConfigSettings>('reset_config_settings');
}
