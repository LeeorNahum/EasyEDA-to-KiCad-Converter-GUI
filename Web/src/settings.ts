/**
 * The choices the window keeps from one launch to the next, so a project's
 * folder is chosen once. Overwrite is left out on purpose: it starts off
 * every time, so nothing is replaced by a choice made days ago.
 */
import type { LibraryMode } from "./bridge.ts";

export interface Settings {
  outputFolder: string;
  mode: LibraryMode;
  symbol: boolean;
  footprint: boolean;
  model: boolean;
  projectRelative: boolean;
}

export const defaultSettings: Settings = {
  outputFolder: "",
  mode: "singlePart",
  symbol: true,
  footprint: true,
  model: true,
  projectRelative: true,
};

/** Saved settings, with anything missing or unreadable at its default. */
export function readSettings(saved: string | null): Settings {
  let value: unknown = null;
  try {
    value = saved === null ? null : JSON.parse(saved);
  } catch {
    value = null;
  }
  if (typeof value !== "object" || value === null) return defaultSettings;
  const record = value as Record<string, unknown>;
  const flag = (key: keyof Settings) =>
    typeof record[key] === "boolean" ? record[key] : defaultSettings[key];
  return {
    outputFolder:
      typeof record.outputFolder === "string" ? record.outputFolder : defaultSettings.outputFolder,
    mode:
      record.mode === "singlePart" || record.mode === "customLibrary"
        ? record.mode
        : defaultSettings.mode,
    symbol: flag("symbol") as boolean,
    footprint: flag("footprint") as boolean,
    model: flag("model") as boolean,
    projectRelative: flag("projectRelative") as boolean,
  };
}
