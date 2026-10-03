import assert from "node:assert/strict";
import { test } from "node:test";

import { defaultSettings, readSettings } from "./settings.ts";

test("saved settings come back as saved", () => {
  const saved = {
    outputFolder: "C:\\Board\\lib",
    mode: "customLibrary",
    symbol: true,
    footprint: false,
    model: false,
    projectRelative: false,
  };
  assert.deepEqual(readSettings(JSON.stringify(saved)), saved);
});

test("anything missing or unreadable is the default", () => {
  assert.deepEqual(readSettings(null), defaultSettings);
  assert.deepEqual(readSettings("not json"), defaultSettings);
  assert.deepEqual(readSettings("[]").mode, defaultSettings.mode);
  assert.deepEqual(
    readSettings(JSON.stringify({ mode: "other", symbol: "yes", outputFolder: 3 })),
    defaultSettings,
  );
  assert.equal(readSettings(JSON.stringify({ overwrite: true })).symbol, true);
});
