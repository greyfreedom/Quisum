#!/usr/bin/env node

import { lstat, readFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const errors = [];
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;
const officialMethods = [
  "behavioral-experiment",
  "decision-reflection",
  "event-debrief",
  "free-reflection",
  "pattern-validation",
  "periodic-review",
  "values-clarification",
];
const requiredDocuments = [
  "CHANGELOG.md",
  "CODE_OF_CONDUCT.md",
  "CONTRIBUTING.md",
  "LICENSE",
  "README.md",
  "SECURITY.md",
  "THIRD_PARTY_NOTICES.md",
  "docs/PROVIDER_COMPATIBILITY.md",
  "docs/RELEASE_PROCESS.md",
  "docs/V1_READINESS.md",
  "docs/guides/HOST_AI.md",
  "docs/guides/INSTALLATION.md",
  "docs/guides/MIGRATIONS.md",
  "docs/guides/PRIVACY_AND_DELETION.md",
  "docs/guides/PROVIDERS.md",
  "docs/guides/RECOVERY.md",
  "release/README.md",
  "release/readiness.json",
  "scripts/check-skills.mjs",
  "scripts/install-skills.mjs",
  "scripts/sync-skills.mjs",
];
const stableGateIds = new Set([
  "clean-install-validation",
  "crisis-resource-source-check",
  "cross-platform-build-automation",
  "independent-security-review",
  "live-provider-compatibility",
  "macos-code-signing",
  "professional-method-review",
  "release-metadata-automation",
  "repository-quality",
  "windows-code-signing",
]);

function fail(message) {
  errors.push(message);
}

function parseArgs(argv) {
  let tag = process.env.GITHUB_REF_TYPE === "tag" ? process.env.GITHUB_REF_NAME : null;
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] !== "--tag" || index + 1 >= argv.length || argv[index + 1].startsWith("--")) {
      fail(`unsupported argument: ${argv[index]}`);
      continue;
    }
    tag = argv[index + 1];
    index += 1;
  }
  return { tag };
}

async function readText(relative) {
  try {
    const file = path.join(root, relative);
    const metadata = await lstat(file);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      fail(`${relative} must be a regular, non-symlink file`);
      return "";
    }
    const text = await readFile(file, "utf8");
    if (text.trim().length === 0) fail(`${relative} must not be empty`);
    return text;
  } catch {
    fail(`${relative} is missing or unreadable`);
    return "";
  }
}

async function readJson(relative) {
  const text = await readText(relative);
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(`${relative} is invalid JSON: ${error.message}`);
    return null;
  }
}

function cargoWorkspaceField(manifest, field) {
  const section = manifest.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/);
  const match = section?.[1].match(new RegExp(`^${field}\\s*=\\s*"([^"]+)"\\s*$`, "m"));
  return match?.[1] ?? null;
}

function lockPackageVersion(lock, name) {
  for (const block of lock.split(/\n(?=\[\[package\]\]\n)/)) {
    if (new RegExp(`^name = "${name}"$`, "m").test(block)) {
      return block.match(/^version = "([^"]+)"$/m)?.[1] ?? null;
    }
  }
  return null;
}

function yamlReview(manifest) {
  const review = manifest.match(/^review:\s*\n((?:^[ \t]+.*\n?)*)/m)?.[1] ?? "";
  const scalar = (key) => review.match(new RegExp(`^[ \\t]+${key}:\\s*(.*?)\\s*$`, "m"))?.[1] ?? null;
  return {
    status: scalar("status"),
    reviewedAt: scalar("reviewed_at"),
    reviewer: scalar("reviewer"),
  };
}

function validDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(parsed.valueOf()) && parsed.toISOString().slice(0, 10) === value;
}

const { tag } = parseArgs(process.argv.slice(2));
for (const document of requiredDocuments) await readText(document);

const cargoManifest = await readText("Cargo.toml");
const cargoLock = await readText("Cargo.lock");
const packageJson = await readJson("apps/desktop/package.json");
const packageLock = await readJson("apps/desktop/package-lock.json");
const tauriConfig = await readJson("apps/desktop/src-tauri/tauri.conf.json");
const readiness = await readJson("release/readiness.json");
const crisisPack = await readJson("resources/crisis/v1.0.0.json");
const readme = await readText("README.md");

const version = cargoWorkspaceField(cargoManifest, "version");
const rustVersion = cargoWorkspaceField(cargoManifest, "rust-version");
const parsedVersion = version?.match(semver) ?? null;
if (!parsedVersion) fail("workspace version must be valid SemVer");
const prerelease = parsedVersion?.[4] != null;
const expectedChannel = prerelease ? "release-candidate" : "stable";
if (parsedVersion && parsedVersion[1] !== "1") {
  fail("the V1 release workflow accepts only major version 1");
}
if (!/^\d+\.\d+$/.test(rustVersion ?? "")) fail("workspace rust-version must be a major.minor baseline");

const alignedVersions = new Map([
  ["apps/desktop/package.json", packageJson?.version],
  ["apps/desktop/package-lock.json", packageLock?.version],
  ["apps/desktop/package-lock.json root package", packageLock?.packages?.[""]?.version],
  ["apps/desktop/src-tauri/tauri.conf.json", tauriConfig?.version],
]);
for (const [file, candidate] of alignedVersions) {
  if (candidate !== version) fail(`${file} version ${String(candidate)} does not match ${String(version)}`);
}
for (const packageName of [
  "quisum-cli",
  "quisum-core",
  "quisum-desktop",
  "quisum-evals",
  "quisum-mcp",
  "quisum-providers",
  "quisum-vault",
]) {
  if (lockPackageVersion(cargoLock, packageName) !== version) {
    fail(`Cargo.lock package ${packageName} does not match workspace version ${String(version)}`);
  }
}
if (tag && tag !== `v${version}`) fail(`tag ${tag} must exactly equal v${String(version)}`);
if (version && !readme.includes(`\`${version}\``)) fail(`README.md must identify the current version ${version}`);

if (!readiness || typeof readiness !== "object" || Array.isArray(readiness)) {
  fail("release/readiness.json must contain an object");
} else {
  if (readiness.schema_version !== "1.0.0") fail("readiness schema_version must be 1.0.0");
  if (readiness.release_version !== version) fail("readiness release_version must match the workspace");
  if (readiness.release_channel !== expectedChannel) {
    fail(`readiness release_channel must be ${expectedChannel}`);
  }
  if (!Array.isArray(readiness.gates)) {
    fail("readiness gates must be an array");
  } else {
    const seen = new Set();
    for (const gate of readiness.gates) {
      if (!gate || typeof gate !== "object" || Array.isArray(gate)) {
        fail("every readiness gate must be an object");
        continue;
      }
      if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(gate.id ?? "")) {
        fail(`invalid readiness gate id: ${String(gate.id)}`);
        continue;
      }
      if (seen.has(gate.id)) fail(`duplicate readiness gate: ${gate.id}`);
      seen.add(gate.id);
      if (!new Set(["open", "complete"]).has(gate.status)) {
        fail(`gate ${gate.id} has invalid status ${String(gate.status)}`);
      }
      if (typeof gate.required_for_stable !== "boolean") {
        fail(`gate ${gate.id} must declare required_for_stable`);
      }
      if (stableGateIds.has(gate.id) && gate.required_for_stable !== true) {
        fail(`stable gate ${gate.id} must remain required_for_stable`);
      }
      if (gate.status === "complete") {
        if (typeof gate.evidence !== "string" || !gate.evidence.trim()) {
          fail(`complete gate ${gate.id} needs a repository evidence path`);
        } else {
          const normalized = path.normalize(gate.evidence);
          const absolute = path.resolve(root, normalized);
          if (path.isAbsolute(gate.evidence) || absolute === root || !absolute.startsWith(`${root}${path.sep}`)) {
            fail(`gate ${gate.id} evidence must stay inside the repository`);
          } else {
            await readText(normalized);
          }
        }
      } else if (gate.evidence !== null) {
        fail(`open gate ${gate.id} must use null evidence rather than a placeholder`);
      }
    }
    for (const id of stableGateIds) {
      if (!seen.has(id)) fail(`missing required stable gate: ${id}`);
    }
    if (!prerelease) {
      for (const gate of readiness.gates) {
        if (gate.required_for_stable && gate.status !== "complete") {
          fail(`stable release is blocked by open gate ${gate.id}`);
        }
      }
    }
  }
}

for (const method of officialMethods) {
  const review = yamlReview(await readText(`methods/${method}/method.yaml`));
  if (prerelease) {
    if (review.status !== "unreviewed" || review.reviewedAt !== "null" || review.reviewer !== "null") {
      fail(`${method} must remain accurately unreviewed in this release candidate`);
    }
  } else if (review.status !== "professional" || review.reviewedAt === "null" || review.reviewer === "null") {
    fail(`${method} requires traceable professional review before stable V1.0`);
  }
}

if (crisisPack) {
  const checked = crisisPack.source_checked_at;
  const due = crisisPack.source_check_due;
  const today = (process.env.QUISUM_RELEASE_DATE ?? new Date().toISOString()).slice(0, 10);
  if (!validDate(checked) || !validDate(due) || checked > due) {
    fail("crisis resource source-check dates are invalid");
  } else if (!validDate(today) || today > due) {
    fail(`crisis resources became stale on ${due}; verify sources before release`);
  }
  if (crisisPack.pack_version !== "1.0.0") fail("unexpected crisis resource pack version");
}

for (const workflow of [".github/workflows/ci.yml", ".github/workflows/release.yml"]) {
  const text = await readText(workflow);
  const toolchains = [...text.matchAll(/^\s+toolchain:\s*([^\s#]+)\s*$/gm)].map((match) => match[1]);
  if (rustVersion && (toolchains.length === 0 || toolchains.some((value) => value !== `${rustVersion}.0`))) {
    fail(`${workflow} must use only the declared Rust ${rustVersion} baseline`);
  }
  const actionReferences = [...text.matchAll(/^\s*-\s+uses:\s*([^\s#]+)\s*$/gm)];
  if (actionReferences.length === 0) fail(`${workflow} must contain pinned action references`);
  for (const match of actionReferences) {
    const reference = match[1].split("@")[1];
    if (!reference || !/^[0-9a-f]{40}$/.test(reference)) {
      fail(`${workflow} action ${match[1]} must be pinned to a full commit SHA`);
    }
  }
}

if (errors.length > 0) {
  for (const error of errors.sort()) console.error(`ERROR ${error}`);
  console.error(`\nRelease checks failed with ${errors.length} error(s).`);
  process.exitCode = 1;
} else {
  console.log(`Release checks passed for ${version} (${expectedChannel}).`);
}
