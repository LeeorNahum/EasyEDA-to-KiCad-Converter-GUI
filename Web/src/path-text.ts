/**
 * Shortens a file path to fit a width while keeping it readable: the first
 * and last parts always stay, then parts are added back from the outside in,
 * and every run of left-out parts becomes one ellipsis.
 */

/** The parts of a path, each keeping the separator that follows it. */
export function pathParts(path: string): string[] {
  return path.match(/[^\\/]*[\\/]|[^\\/]+$/g) ?? [path];
}

function render(parts: string[], kept: Set<number>): string {
  let text = "";
  let skipping = false;
  parts.forEach((part, index) => {
    if (kept.has(index)) {
      text += part;
      skipping = false;
    } else if (!skipping) {
      const separator = part.match(/[\\/]$/)?.[0] ?? "";
      text += `…${separator}`;
      skipping = true;
    }
  });
  return text;
}

/**
 * The longest outside-in shortening of `path` that `fits`. When even the
 * first and last parts do not fit together, they are returned as they are
 * and the caller clips them.
 */
export function compactPath(path: string, fits: (text: string) => boolean): string {
  if (fits(path)) {
    return path;
  }
  const parts = pathParts(path);
  const last = parts.length - 1;
  const kept = new Set([0, last]);
  let best = render(parts, kept);
  if (!fits(best)) {
    return best;
  }
  // Outside in: the second part, the second to last, the third, and so on.
  // A part that does not fit is skipped, since a shorter one further in may.
  for (let step = 1; step <= last - step; step += 1) {
    for (const index of new Set([step, last - step])) {
      kept.add(index);
      const candidate = render(parts, kept);
      if (fits(candidate)) {
        best = candidate;
      } else {
        kept.delete(index);
      }
    }
  }
  return best;
}
