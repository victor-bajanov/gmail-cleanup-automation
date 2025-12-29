/**
 * Accessibility utilities
 */

// Announce messages to screen readers
let announcer: HTMLElement | null = null;

export function initAnnouncer(): void {
  if (announcer) return;

  announcer = document.createElement('div');
  announcer.setAttribute('aria-live', 'polite');
  announcer.setAttribute('aria-atomic', 'true');
  announcer.setAttribute('role', 'status');
  announcer.className = 'sr-only';
  document.body.appendChild(announcer);
}

export function announce(message: string, priority: 'polite' | 'assertive' = 'polite'): void {
  if (!announcer) {
    initAnnouncer();
  }

  if (announcer) {
    announcer.setAttribute('aria-live', priority);
    // Clear and set to trigger announcement
    announcer.textContent = '';
    // Use setTimeout to ensure the DOM update triggers the announcement
    setTimeout(() => {
      if (announcer) {
        announcer.textContent = message;
      }
    }, 100);
  }
}

// Focus management utilities
export function focusFirst(container: HTMLElement): void {
  const focusable = getFocusableElements(container);
  if (focusable.length > 0) {
    focusable[0].focus();
  }
}

export function focusLast(container: HTMLElement): void {
  const focusable = getFocusableElements(container);
  if (focusable.length > 0) {
    focusable[focusable.length - 1].focus();
  }
}

export function trapFocus(container: HTMLElement, event: KeyboardEvent): void {
  const focusable = getFocusableElements(container);
  if (focusable.length === 0) return;

  const first = focusable[0];
  const last = focusable[focusable.length - 1];

  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

export function getFocusableElements(container: HTMLElement): HTMLElement[] {
  const selector = [
    'button:not([disabled])',
    'a[href]',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
  ].join(', ');

  return Array.from(container.querySelectorAll<HTMLElement>(selector));
}

// Keyboard navigation helpers
export function handleArrowNavigation(
  event: KeyboardEvent,
  items: HTMLElement[],
  currentIndex: number,
  options: {
    vertical?: boolean;
    horizontal?: boolean;
    wrap?: boolean;
  } = {}
): number {
  const { vertical = true, horizontal = false, wrap = true } = options;

  let newIndex = currentIndex;
  const last = items.length - 1;

  switch (event.key) {
    case 'ArrowUp':
      if (vertical) {
        event.preventDefault();
        newIndex = wrap
          ? currentIndex <= 0 ? last : currentIndex - 1
          : Math.max(0, currentIndex - 1);
      }
      break;
    case 'ArrowDown':
      if (vertical) {
        event.preventDefault();
        newIndex = wrap
          ? currentIndex >= last ? 0 : currentIndex + 1
          : Math.min(last, currentIndex + 1);
      }
      break;
    case 'ArrowLeft':
      if (horizontal) {
        event.preventDefault();
        newIndex = wrap
          ? currentIndex <= 0 ? last : currentIndex - 1
          : Math.max(0, currentIndex - 1);
      }
      break;
    case 'ArrowRight':
      if (horizontal) {
        event.preventDefault();
        newIndex = wrap
          ? currentIndex >= last ? 0 : currentIndex + 1
          : Math.min(last, currentIndex + 1);
      }
      break;
    case 'Home':
      event.preventDefault();
      newIndex = 0;
      break;
    case 'End':
      event.preventDefault();
      newIndex = last;
      break;
  }

  if (newIndex !== currentIndex && items[newIndex]) {
    items[newIndex].focus();
  }

  return newIndex;
}

// Skip link helper
export function createSkipLink(targetId: string, label: string = 'Skip to main content'): HTMLAnchorElement {
  const link = document.createElement('a');
  link.href = `#${targetId}`;
  link.className = 'sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:px-4 focus:py-2 focus:bg-primary-600 focus:text-white focus:rounded';
  link.textContent = label;
  return link;
}

// Reduced motion detection
export function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

// High contrast detection
export function prefersHighContrast(): boolean {
  return window.matchMedia('(prefers-contrast: more)').matches;
}

// Color scheme preference
export function prefersDarkMode(): boolean {
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}
