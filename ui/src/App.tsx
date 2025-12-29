import { Component, createEffect, onMount, Show, Switch, Match } from 'solid-js';
import { auth, navigation, errorState, ui, scan } from './stores/app';
import * as api from './lib/api';
import { setupEventListeners } from './lib/events';

// Views
import AuthView from './components/views/AuthView';
import ScanView from './components/views/ScanView';
import ReviewView from './components/views/ReviewView';
import FiltersView from './components/views/FiltersView';
import CoverageView from './components/views/CoverageView';
import SettingsView from './components/views/SettingsView';

// Layout components
import Sidebar from './components/layout/Sidebar';
import Header from './components/layout/Header';
import ErrorToast from './components/ui/ErrorToast';
import SkipLink from './components/ui/SkipLink';
import { initGlobalAnnouncer } from './components/ui/ScreenReaderAnnouncer';

const App: Component = () => {
  // Apply dark mode class to document
  createEffect(() => {
    if (ui.isDarkMode()) {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  });

  // Check auth status on mount
  onMount(async () => {
    // Initialize accessibility features
    initGlobalAnnouncer();

    // Set up event listeners
    const cleanup = await setupEventListeners({
      scanProgress: (progress) => {
        scan.setProgress(progress);
      },
      error: (error) => {
        errorState.add(error);
      },
    });

    // Check auth status
    try {
      const status = await api.checkAuthStatus();
      auth.setStatus(status);

      if (status.authenticated) {
        await api.initializeClient();
        navigation.goTo('scan');
      }
    } catch (e) {
      console.error('Failed to check auth status:', e);
    }

    // Cleanup on unmount (though this won't happen in Tauri)
    return cleanup;
  });

  return (
    <div class="flex h-screen bg-gray-50 dark:bg-gray-900">
      {/* Skip link for accessibility */}
      <SkipLink targetId="main-content" label="Skip to main content" />

      {/* Sidebar - shown when authenticated */}
      <Show when={auth.isAuthenticated()}>
        <Sidebar />
      </Show>

      {/* Main content area */}
      <div class="flex-1 flex flex-col overflow-hidden">
        {/* Header */}
        <Header />

        {/* Main content */}
        <main
          id="main-content"
          class="flex-1 overflow-auto p-6"
          tabIndex={-1}
          role="main"
          aria-label="Main content"
        >
          <Switch fallback={<AuthView />}>
            <Match when={navigation.currentView() === 'auth'}>
              <AuthView />
            </Match>
            <Match when={navigation.currentView() === 'scan'}>
              <ScanView />
            </Match>
            <Match when={navigation.currentView() === 'review'}>
              <ReviewView />
            </Match>
            <Match when={navigation.currentView() === 'filters'}>
              <FiltersView />
            </Match>
            <Match when={navigation.currentView() === 'coverage'}>
              <CoverageView />
            </Match>
            <Match when={navigation.currentView() === 'settings'}>
              <SettingsView />
            </Match>
          </Switch>
        </main>
      </div>

      {/* Error toast */}
      <Show when={errorState.last()}>
        <ErrorToast
          error={errorState.last()!}
          onDismiss={() => errorState.dismiss()}
        />
      </Show>
    </div>
  );
};

export default App;
