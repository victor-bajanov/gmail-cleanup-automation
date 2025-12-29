import { Component, For } from 'solid-js';
import { navigation, review, ui } from '../../stores/app';
import type { AppView } from '../../types';

interface NavItem {
  id: AppView;
  label: string;
  icon: string;
  badge?: () => number | null;
}

const navItems: NavItem[] = [
  { id: 'scan', label: 'Scan', icon: '📧' },
  { id: 'review', label: 'Review', icon: '✓', badge: () => review.undecidedCount() || null },
  { id: 'filters', label: 'Filters', icon: '🔧' },
  { id: 'coverage', label: 'Coverage', icon: '📊' },
];

const Sidebar: Component = () => {
  return (
    <aside
      class="w-64 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col"
      classList={{ hidden: !ui.isSidebarOpen() }}
    >
      {/* Logo/Title */}
      <div class="p-4 border-b border-gray-200 dark:border-gray-700">
        <h1 class="text-xl font-bold text-gray-900 dark:text-white">
          Gmail Cleanup
        </h1>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Filter Management
        </p>
      </div>

      {/* Navigation */}
      <nav class="flex-1 p-4">
        <ul class="space-y-1">
          <For each={navItems}>
            {(item) => (
              <li>
                <button
                  onClick={() => navigation.goTo(item.id)}
                  class="w-full flex items-center gap-3 px-3 py-2 rounded-lg transition-colors"
                  classList={{
                    'bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-300':
                      navigation.currentView() === item.id,
                    'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700':
                      navigation.currentView() !== item.id,
                  }}
                >
                  <span class="text-lg">{item.icon}</span>
                  <span class="font-medium">{item.label}</span>
                  {item.badge && item.badge() && (
                    <span class="ml-auto badge-info">{item.badge()}</span>
                  )}
                </button>
              </li>
            )}
          </For>
        </ul>
      </nav>

      {/* Footer */}
      <div class="p-4 border-t border-gray-200 dark:border-gray-700">
        <button
          onClick={() => navigation.goTo('settings')}
          class="w-full flex items-center gap-2 px-3 py-2 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 transition-colors"
          classList={{
            'bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100':
              navigation.currentView() === 'settings',
          }}
        >
          <span>⚙️</span>
          <span>Settings</span>
        </button>
      </div>
    </aside>
  );
};

export default Sidebar;
