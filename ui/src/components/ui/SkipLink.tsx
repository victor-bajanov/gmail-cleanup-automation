import { Component } from 'solid-js';

interface SkipLinkProps {
  targetId: string;
  label?: string;
}

/**
 * Skip link for keyboard navigation
 * Allows users to skip repetitive navigation and go directly to main content
 */
const SkipLink: Component<SkipLinkProps> = (props) => {
  const handleClick = (e: MouseEvent) => {
    e.preventDefault();
    const target = document.getElementById(props.targetId);
    if (target) {
      target.focus();
      target.scrollIntoView();
    }
  };

  return (
    <a
      href={`#${props.targetId}`}
      onClick={handleClick}
      class="
        sr-only focus:not-sr-only
        focus:absolute focus:top-4 focus:left-4 focus:z-50
        focus:px-4 focus:py-2
        focus:bg-primary-600 focus:text-white
        focus:rounded-lg focus:shadow-lg
        focus:outline-none focus:ring-2 focus:ring-primary-500 focus:ring-offset-2
        transition-all
      "
    >
      {props.label || 'Skip to main content'}
    </a>
  );
};

export default SkipLink;
