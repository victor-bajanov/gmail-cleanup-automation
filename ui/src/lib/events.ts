/**
 * Event listeners for Tauri backend events
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ScanProgress,
  FilterProgress,
  ErrorEvent,
  ClusterEvent,
  AuthStatus,
} from '../types';

// Event handler types
export type ScanProgressHandler = (progress: ScanProgress) => void;
export type FilterProgressHandler = (progress: FilterProgress) => void;
export type ErrorHandler = (error: ErrorEvent) => void;
export type ClusterEventHandler = (event: ClusterEvent) => void;
export type AuthEventHandler = (status: AuthStatus) => void;

// ============ Scan Events ============

export async function onScanProgress(handler: ScanProgressHandler): Promise<UnlistenFn> {
  return listen<ScanProgress>('scan:progress', (event) => {
    handler(event.payload);
  });
}

// ============ Filter Events ============

export async function onFilterProgress(handler: FilterProgressHandler): Promise<UnlistenFn> {
  return listen<FilterProgress>('filter:progress', (event) => {
    handler(event.payload);
  });
}

// ============ Auth Events ============

export async function onAuthStatus(handler: AuthEventHandler): Promise<UnlistenFn> {
  return listen<AuthStatus>('auth:status', (event) => {
    handler(event.payload);
  });
}

// ============ Cluster Events ============

export async function onClusterEvent(handler: ClusterEventHandler): Promise<UnlistenFn> {
  return listen<ClusterEvent>('cluster:event', (event) => {
    handler(event.payload);
  });
}

// ============ Error Events ============

export async function onError(handler: ErrorHandler): Promise<UnlistenFn> {
  return listen<ErrorEvent>('error', (event) => {
    handler(event.payload);
  });
}

// ============ Label Events ============

export interface LabelProgress {
  operation: 'fetching' | 'creating' | 'applying' | 'complete';
  current: number;
  total: number;
  label_name?: string;
}

export type LabelProgressHandler = (progress: LabelProgress) => void;

export async function onLabelProgress(handler: LabelProgressHandler): Promise<UnlistenFn> {
  return listen<LabelProgress>('label:progress', (event) => {
    handler(event.payload);
  });
}

// ============ Utility: Combined Listener ============

export interface EventListeners {
  scanProgress?: ScanProgressHandler;
  filterProgress?: FilterProgressHandler;
  labelProgress?: LabelProgressHandler;
  clusterEvent?: ClusterEventHandler;
  authStatus?: AuthEventHandler;
  error?: ErrorHandler;
}

/**
 * Sets up multiple event listeners at once.
 * Returns a cleanup function that unregisters all listeners.
 */
export async function setupEventListeners(listeners: EventListeners): Promise<() => void> {
  const unlisteners: UnlistenFn[] = [];

  if (listeners.scanProgress) {
    unlisteners.push(await onScanProgress(listeners.scanProgress));
  }

  if (listeners.filterProgress) {
    unlisteners.push(await onFilterProgress(listeners.filterProgress));
  }

  if (listeners.labelProgress) {
    unlisteners.push(await onLabelProgress(listeners.labelProgress));
  }

  if (listeners.clusterEvent) {
    unlisteners.push(await onClusterEvent(listeners.clusterEvent));
  }

  if (listeners.authStatus) {
    unlisteners.push(await onAuthStatus(listeners.authStatus));
  }

  if (listeners.error) {
    unlisteners.push(await onError(listeners.error));
  }

  // Return cleanup function
  return () => {
    unlisteners.forEach((unlisten) => unlisten());
  };
}
