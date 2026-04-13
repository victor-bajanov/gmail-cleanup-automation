export interface LabelSegment {
  text: string;
  highlighted: boolean;
}

/**
 * Strip `prefix/` from the start of a label.
 * Only strips when there are segments after the prefix.
 * Empty prefix returns label unchanged.
 */
export function stripPrefix(label: string, prefix: string): string {
  if (!prefix) return label;
  const token = `${prefix}/`;
  if (label.startsWith(token) && label.length > token.length) {
    return label.slice(token.length);
  }
  return label;
}

/**
 * Split each label by `/` into segments.
 * Mark segments as highlighted where not ALL labels share the same value
 * at that position (or a label is shorter than others).
 */
export function diffLabelSegments(labels: string[]): LabelSegment[][] {
  const split = labels.map((l) => l.split('/'));
  const maxLen = Math.max(...split.map((s) => s.length));

  // For each segment index, determine if all labels agree
  const allSame: boolean[] = [];
  for (let i = 0; i < maxLen; i++) {
    if (labels.length <= 1) {
      allSame.push(true);
    } else {
      const first = split[0]?.[i];
      const same = split.every((s) => i < s.length && s[i] === first);
      allSame.push(same);
    }
  }

  return split.map((segments) =>
    segments.map((text, i) => ({
      text,
      highlighted: !allSame[i],
    })),
  );
}

/**
 * Look up a label ID in a name map, strip the prefix, and fall back to raw ID.
 */
export function resolveLabelId(
  id: string,
  idToName: Record<string, string>,
  prefix: string,
): string {
  const name = Object.hasOwn(idToName, id) ? idToName[id] : undefined;
  if (name !== undefined) {
    return stripPrefix(name, prefix);
  }
  return id;
}
