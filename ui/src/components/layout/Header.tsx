import { Component, Show } from 'solid-js';
import { auth, navigation, ui, review } from '../../stores/app';
import * as api from '../../lib/api';

const viewTitles: Record<string, string> = {
  auth: 'Authentication',
  scan: 'Scan Emails',
  review: 'Review Clusters',
  filters: 'Filter Visualization',
  coverage: 'Coverage Analysis',
  settings: 'Settings',
};

const Header: Component = () => {
  const handleLogout = async () => {
    try {
      await api.logout();
      auth.setStatus(null);
      navigation.goTo('auth');
    } catch (e) {
      console.error('Logout failed:', e);
    }
  };

  return (
    <header class="bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 px-6 py-4">
      <div class="flex items-center justify-between">
        {/* Left: Menu button + Title */}
        <div class="flex items-center gap-4">
          <Show when={auth.isAuthenticated()}>
            <button
              onClick={() => ui.toggleSidebar()}
              class="p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
              aria-label="Toggle sidebar"
            >
              <svg
                class="w-5 h-5 text-gray-600 dark:text-gray-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M4 6h16M4 12h16M4 18h16"
                />
              </svg>
            </button>
          </Show>

          <div>
            <h2 class="text-lg font-semibold text-gray-900 dark:text-white">
              {viewTitles[navigation.currentView()] || 'Gmail Cleanup'}
            </h2>
            <Show when={navigation.currentView() === 'review' && review.summary()}>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                {review.summary()!.remaining} of {review.summary()!.total} remaining
              </p>
            </Show>
          </div>
        </div>

        {/* Right: User menu */}
        <div class="flex items-center gap-4">
          {/* Dark mode toggle */}
          <button
            onClick={() => ui.toggleDarkMode()}
            class="p-2 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            aria-label="Toggle dark mode"
          >
            <Show
              when={ui.isDarkMode()}
              fallback={
                <svg
                  class="w-5 h-5 text-gray-600 dark:text-gray-400"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z"
                  />
                </svg>
              }
            >
              <svg
                class="w-5 h-5 text-gray-600 dark:text-gray-400"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z"
                />
              </svg>
            </Show>
          </button>

          {/* User info & logout */}
          <Show when={auth.isAuthenticated()}>
            <div class="flex items-center gap-3">
              <span class="text-sm text-gray-600 dark:text-gray-400">
                {auth.email()}
              </span>
              <button
                onClick={handleLogout}
                class="btn-ghost text-sm"
              >
                Logout
              </button>
            </div>
          </Show>
        </div>
      </div>
    </header>
  );
};

export default Header;
