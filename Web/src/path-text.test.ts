import assert from "node:assert/strict";
import { test } from "node:test";

import { compactPath, pathParts } from "./path-text.ts";

const within = (limit: number) => (text: string) => text.length <= limit;

test("a path splits into parts that keep their separators", () => {
  assert.deepEqual(pathParts("C:\\Users\\me\\lib"), ["C:\\", "Users\\", "me\\", "lib"]);
  assert.deepEqual(pathParts("${KIPRJMOD}/a/b.3dshapes"), ["${KIPRJMOD}/", "a/", "b.3dshapes"]);
});

test("a path that fits is unchanged", () => {
  assert.equal(compactPath("C:\\a\\b", within(50)), "C:\\a\\b");
});

test("parts are kept from the outside in", () => {
  const path = "C:\\Users\\Laptop\\Documents\\Kicad\\easyeda2kicad\\Part_C1";
  assert.equal(compactPath(path, within(30)), "C:\\Users\\Laptop\\\u2026\\Part_C1");
  assert.equal(compactPath(path, within(22)), "C:\\Users\\\u2026\\Part_C1");
  assert.equal(compactPath(path, within(15)), "C:\\\u2026\\Part_C1");
});

test("a long part is left out while shorter ones around it still fit", () => {
  const path = "C:\\a\\very-long-folder-name\\b\\end";
  assert.equal(compactPath(path, within(14)), "C:\\a\\\u2026\\b\\end");
});

test("the end points stay even when they do not fit", () => {
  assert.equal(
    compactPath("C:\\a\\b\\a-very-long-name", within(5)),
    "C:\\\u2026\\a-very-long-name",
  );
});
