/**
 * The library name field and how it follows the part.
 *
 * In Single Part Folder mode every part gets its own folder, so the name
 * follows the part: a new part replaces it with that part's suggested name,
 * and an edit holds only while the same part stays in the field. In Custom
 * Library mode the name is the shared library several parts go into, so it
 * stays as typed and is only filled in when empty.
 */
import type { LibraryMode } from "./bridge.ts";

export interface LibraryName {
  value: string;
  /** Whether the user typed the current value. */
  edited: boolean;
  /** The part whose suggestion the value came from or was edited over. */
  part: string;
}

export const emptyLibraryName: LibraryName = { value: "", edited: false, part: "" };

/** A suggested name arrived for `part`, or the mode changed. */
export function suggest(
  state: LibraryName,
  mode: LibraryMode,
  part: string,
  suggestion: string,
): LibraryName {
  if (suggestion === "") {
    return state;
  }
  if (mode === "singlePart") {
    if (state.part !== part || !state.edited) {
      return { value: suggestion, edited: false, part };
    }
    return state;
  }
  if (state.value.trim() === "") {
    return { value: suggestion, edited: false, part };
  }
  return state;
}

/** The user typed in the field. */
export function edit(value: string, part: string, suggestion: string): LibraryName {
  return { value, edited: value !== suggestion, part };
}
