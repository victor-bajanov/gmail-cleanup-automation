import { Component, createEffect, createSignal } from 'solid-js';

interface ScreenReaderAnnouncerProps {
  message?: string;
  priority?: 'polite' | 'assertive';
}

/**
 * Screen reader announcer component
 * Uses ARIA live regions to announce dynamic content changes
 */
const ScreenReaderAnnouncer: Component<ScreenReaderAnnouncerProps> = (props) => {
  const [announcement, setAnnouncement] = createSignal('');

  // When message changes, trigger announcement
  createEffect(() => {
    const message = props.message;
    if (message) {
      // Clear first to ensure re-announcement of same message
      setAnnouncement('');
      // Small delay to ensure the clear is processed
      setTimeout(() => setAnnouncement(message), 50);
    }
  });

  return (
    <div
      role="status"
      aria-live={props.priority || 'polite'}
      aria-atomic="true"
      class="sr-only"
    >
      {announcement()}
    </div>
  );
};

export default ScreenReaderAnnouncer;

// Hook for programmatic announcements
let globalAnnouncer: HTMLElement | null = null;

export function initGlobalAnnouncer(): void {
  if (globalAnnouncer) return;

  globalAnnouncer = document.createElement('div');
  globalAnnouncer.setAttribute('role', 'status');
  globalAnnouncer.setAttribute('aria-live', 'polite');
  globalAnnouncer.setAttribute('aria-atomic', 'true');
  globalAnnouncer.className = 'sr-only';
  document.body.appendChild(globalAnnouncer);
}

export function announceToScreenReader(message: string, priority: 'polite' | 'assertive' = 'polite'): void {
  if (!globalAnnouncer) {
    initGlobalAnnouncer();
  }

  if (globalAnnouncer) {
    globalAnnouncer.setAttribute('aria-live', priority);
    globalAnnouncer.textContent = '';
    setTimeout(() => {
      if (globalAnnouncer) {
        globalAnnouncer.textContent = message;
      }
    }, 100);
  }
}
