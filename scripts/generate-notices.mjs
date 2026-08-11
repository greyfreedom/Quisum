#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const output = path.join(root, "THIRD_PARTY_NOTICES.md");
const check = process.argv.length === 3 && process.argv[2] === "--check";
if (process.argv.length > (check ? 3 : 2)) {
  console.error("Usage: node scripts/generate-notices.mjs [--check]");
  process.exit(2);
}

const cargoLock = await readFile(path.join(root, "Cargo.lock"), "utf8");
const packageLockText = await readFile(path.join(root, "apps/desktop/package-lock.json"), "utf8");
const packageLock = JSON.parse(packageLockText);
const metadataRun = spawnSync("cargo", ["metadata", "--locked", "--format-version", "1"], {
  cwd: root,
  encoding: "utf8",
  maxBuffer: 64 * 1024 * 1024,
});
if (metadataRun.status !== 0) {
  console.error(metadataRun.stderr.trim() || "cargo metadata failed");
  process.exit(metadataRun.status ?? 1);
}
const metadata = JSON.parse(metadataRun.stdout);
const workspace = new Set(metadata.workspace_members);
const rows = [];

function normalizeLicense(value, licenseFile) {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (typeof licenseFile === "string" && licenseFile.trim()) return `See ${path.basename(licenseFile)}`;
  return "Not declared in lock metadata";
}

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

for (const pkg of metadata.packages) {
  if (workspace.has(pkg.id)) continue;
  rows.push({
    ecosystem: "Rust",
    name: pkg.name,
    version: pkg.version,
    license: normalizeLicense(pkg.license, pkg.license_file),
  });
}

for (const [location, pkg] of Object.entries(packageLock.packages ?? {})) {
  if (!location || !location.includes("node_modules/") || typeof pkg !== "object" || !pkg.version) continue;
  const marker = location.lastIndexOf("node_modules/");
  const name = location.slice(marker + "node_modules/".length);
  rows.push({
    ecosystem: "npm",
    name,
    version: pkg.version,
    license: normalizeLicense(pkg.license, null),
  });
}

const unique = [...new Map(rows.map((row) => [`${row.ecosystem}\0${row.name}\0${row.version}\0${row.license}`, row])).values()]
  .sort((left, right) => {
    const leftKey = `${left.ecosystem}\0${left.name}\0${left.version}`;
    const rightKey = `${right.ecosystem}\0${right.name}\0${right.version}`;
    return leftKey < rightKey ? -1 : leftKey > rightKey ? 1 : 0;
  });
const undeclared = unique.filter((row) => row.license === "Not declared in lock metadata");
if (undeclared.length > 0) {
  for (const row of undeclared) console.error(`Missing license metadata: ${row.ecosystem} ${row.name} ${row.version}`);
  process.exit(1);
}
const fingerprint = createHash("sha256").update(cargoLock).update("\0").update(packageLockText).digest("hex");
const sections = [
  "# Third-party notices",
  "",
  "Quisum is licensed under Apache-2.0. Its distributable builds include third-party packages under their own terms. This generated inventory is informational and does not replace those license texts or legal advice.",
  "",
  `Lockfile fingerprint (SHA-256): \`${fingerprint}\``,
  "",
  "Regenerate with `node scripts/generate-notices.mjs`; verify with `node scripts/generate-notices.mjs --check`.",
  "",
];
for (const ecosystem of ["Rust", "npm"]) {
  sections.push(`## ${ecosystem} packages`, "", "| Package | Version | Declared license |", "| --- | --- | --- |");
  for (const row of unique.filter((candidate) => candidate.ecosystem === ecosystem)) {
    sections.push(`| ${escapeCell(row.name)} | ${escapeCell(row.version)} | ${escapeCell(row.license)} |`);
  }
  sections.push("");
}
const generated = sections.join("\n");

if (check) {
  const current = await readFile(output, "utf8").catch(() => "");
  if (current !== generated) {
    console.error("THIRD_PARTY_NOTICES.md is stale; run node scripts/generate-notices.mjs");
    process.exit(1);
  }
  console.log(`Third-party notices are current (${unique.length} packages).`);
} else {
  await writeFile(output, generated, "utf8");
  console.log(`Generated THIRD_PARTY_NOTICES.md for ${unique.length} packages.`);
}
