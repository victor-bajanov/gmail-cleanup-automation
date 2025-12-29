/**
 * Main application store using SolidJS reactivity
 */

import { createSignal, createMemo } from 'solid-js';
import type {
  AppView,
  AuthStatus,
  ScanProgress,
  ReviewSummary,
  ClusterView,
  FilterComparison,
  CoverageAnalysis,
  ErrorEvent,
} from '../types';

// ============ Auth State ============

const [authStatus, setAuthStatus] = createSignal<AuthStatus | null>(null);
const [isAuthenticating, setIsAuthenticating] = createSignal(false);

export const auth = {
  status: authStatus,
  isAuthenticating,
  setStatus: setAuthStatus,
  setAuthenticating: setIsAuthenticating,
  isAuthenticated: createMemo(() => authStatus()?.authenticated ?? false),
  email: createMemo(() => authStatus()?.email ?? null),
};

// ============ Navigation State ============

const [currentView, setCurrentView] = createSignal<AppView>('auth');

export const navigation = {
  currentView,
  setView: setCurrentView,
  goTo: (view: AppView) => setCurrentView(view),
};

// ============ Scan State ============

const [scanProgress, setScanProgress] = createSignal<ScanProgress | null>(null);
const [isScanning, setIsScanning] = createSignal(false);
const [scanError, setScanError] = createSignal<string | null>(null);

export const scan = {
  progress: scanProgress,
  isScanning,
  error: scanError,
  setProgress: setScanProgress,
  setScanning: setIsScanning,
  setError: setScanError,
  reset: () => {
    setScanProgress(null);
    setIsScanning(false);
    setScanError(null);
  },
};

// ============ Cluster Review State ============

const [clusters, setClusters] = createSignal<ClusterView[]>([]);
const [currentClusterIndex, setCurrentClusterIndex] = createSignal<number | null>(null);
const [reviewSummary, setReviewSummary] = createSignal<ReviewSummary | null>(null);

export const review = {
  clusters,
  currentIndex: currentClusterIndex,
  summary: reviewSummary,
  setClusters,
  setCurrentIndex: setCurrentClusterIndex,
  setSummary: setReviewSummary,
  currentCluster: createMemo(() => {
    const idx = currentClusterIndex();
    const list = clusters();
    if (idx !== null && idx >= 0 && idx < list.length) {
      return list[idx];
    }
    return null;
  }),
  undecidedCount: createMemo(() => {
    return clusters().filter((c) => !c.decided).length;
  }),
  reset: () => {
    setClusters([]);
    setCurrentClusterIndex(null);
    setReviewSummary(null);
  },
};

// ============ Filter State ============

const [filterComparison, setFilterComparison] = createSignal<FilterComparison | null>(null);
const [isLoadingFilters, setIsLoadingFilters] = createSignal(false);

export const filters = {
  comparison: filterComparison,
  isLoading: isLoadingFilters,
  setComparison: setFilterComparison,
  setLoading: setIsLoadingFilters,
  existingFilters: createMemo(() => filterComparison()?.existing ?? []),
  proposedFilters: createMemo(() => filterComparison()?.proposed ?? []),
  summary: createMemo(() => filterComparison()?.summary ?? null),
  reset: () => {
    setFilterComparison(null);
    setIsLoadingFilters(false);
  },
};

// ============ Coverage State ============

const [coverageAnalysis, setCoverageAnalysis] = createSignal<CoverageAnalysis | null>(null);
const [isAnalyzingCoverage, setIsAnalyzingCoverage] = createSignal(false);

export const coverage = {
  analysis: coverageAnalysis,
  isAnalyzing: isAnalyzingCoverage,
  setAnalysis: setCoverageAnalysis,
  setAnalyzing: setIsAnalyzingCoverage,
  coveragePercentage: createMemo(() => coverageAnalysis()?.coverage_percentage ?? 0),
  reset: () => {
    setCoverageAnalysis(null);
    setIsAnalyzingCoverage(false);
  },
};

// ============ Error State ============

const [errors, setErrors] = createSignal<ErrorEvent[]>([]);
const [lastError, setLastError] = createSignal<ErrorEvent | null>(null);

export const errorState = {
  all: errors,
  last: lastError,
  add: (error: ErrorEvent) => {
    setLastError(error);
    setErrors((prev) => [...prev, error]);
  },
  dismiss: () => setLastError(null),
  clear: () => {
    setErrors([]);
    setLastError(null);
  },
};

// ============ UI State ============

const [isDarkMode, setIsDarkMode] = createSignal(
  typeof window !== 'undefined' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
);
const [isSidebarOpen, setIsSidebarOpen] = createSignal(true);

export const ui = {
  isDarkMode,
  setDarkMode: setIsDarkMode,
  toggleDarkMode: () => setIsDarkMode((prev) => !prev),
  isSidebarOpen,
  setSidebarOpen: setIsSidebarOpen,
  toggleSidebar: () => setIsSidebarOpen((prev) => !prev),
};

// ============ Global Reset ============

export function resetAllState() {
  auth.setStatus(null);
  auth.setAuthenticating(false);
  navigation.setView('auth');
  scan.reset();
  review.reset();
  filters.reset();
  coverage.reset();
  errorState.clear();
}
