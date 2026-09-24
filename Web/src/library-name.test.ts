import assert from "node:assert/strict";
import { test } from "node:test";

import { edit, emptyLibraryName, suggest } from "./library-name.ts";

test("single part mode follows the part", () => {
  const first = suggest(emptyLibraryName, "singlePart", "C1", "Part_C1");
  assert.equal(first.value, "Part_C1");
  const edited = edit("Mine", "C1", "Part_C1");
  assert.equal(suggest(edited, "singlePart", "C1", "Part_C1").value, "Mine");
  assert.equal(suggest(edited, "singlePart", "C2", "Other_C2").value, "Other_C2");
});

test("custom library mode keeps a typed name across parts", () => {
  const typed = edit("Passives", "C1", "Part_C1");
  assert.equal(suggest(typed, "customLibrary", "C2", "Other_C2").value, "Passives");
  assert.equal(suggest(emptyLibraryName, "customLibrary", "C2", "Other_C2").value, "Other_C2");
});

test("an edit back to the suggestion counts as not edited", () => {
  assert.equal(edit("Part_C1", "C1", "Part_C1").edited, false);
});
