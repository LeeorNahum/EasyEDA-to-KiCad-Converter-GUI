/**
 * Fails when any text file in the repository contains an em dash or an en
 * dash, written as the character or as an HTML entity.
 */
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { encoding: "utf8" }).trim();
const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard"], {
  cwd: root,
  encoding: "utf8",
})
  .split("\n")
  .filter(
    (file) =>
      file !== "" && !/\.(ico|png|exe|lock)$/.test(file) && !file.endsWith("pnpm-lock.yaml"),
  );

// Built from parts so this file does not match its own pattern.
const dashes = [0x2014, 0x2013].map((code) => String.fromCharCode(code));
const entities = ["mdash;", "#8212;", "#x2014;", "ndash;", "#8211;", "#x2013;"].map(
  (name) => "&" + name,
);
const pattern = new RegExp([...dashes, ...entities].join("|"), "i");
const offenders: string[] = [];
for (const file of files) {
  let text: string;
  try {
    text = readFileSync(join(root, file), "utf8");
  } catch {
    continue;
  }
  text.split("\n").forEach((line, index) => {
    if (pattern.test(line)) {
      offenders.push(`${file}:${index + 1}`);
    }
  });
}

if (offenders.length > 0) {
  console.error(`Em or en dashes found:\n${offenders.join("\n")}`);
  process.exit(1);
}
console.log(`No em or en dashes in ${files.length} files.`);
