#!/usr/bin/env node

import { chmod, cp, lstat, mkdir, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const expectedSkills = [
  "quisum-free-reflection",
  "quisum-event-debrief",
  "quisum-values-clarification",
  "quisum-decision-reflection",
  "quisum-pattern-validation",
  "quisum-behavioral-experiment",
  "quisum-periodic-review",
  "quisum-profile-curation",
];

function usage(message) {
  if (message) console.error(`ERROR ${message}`);
  console.error("Usage: node scripts/package-cli.mjs --binary <path> --output <directory> --platform <label>");
  process.exit(2);
}

function parseArgs(argv) {
  const result = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!["--binary", "--output", "--platform"].includes(key) || !value || value.startsWith("--")) {
      usage(`invalid argument ${key ?? ""}`);
    }
    result[key.slice(2)] = value;
  }
  if (!result.binary || !result.output || !result.platform) usage("all arguments are required");
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(result.platform)) usage("platform label is invalid");
  return result;
}

function isInside(candidate, directory) {
  const relative = path.relative(directory, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

async function requireRegularFile(file, label) {
  const metadata = await lstat(file).catch(() => null);
  if (!metadata?.isFile() || metadata.isSymbolicLink()) usage(`${label} must be a regular, non-symlink file`);
}

async function requireRegularTree(directory) {
  const metadata = await lstat(directory).catch(() => null);
  if (!metadata?.isDirectory() || metadata.isSymbolicLink()) usage(`${directory} must be a regular directory`);
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (/\r|\n/.test(entry.name)) usage("package source names must not contain newlines");
    const source = path.join(directory, entry.name);
    const entryMetadata = await lstat(source);
    if (entryMetadata.isSymbolicLink()) usage(`package source must not contain symlinks: ${source}`);
    if (entryMetadata.isDirectory()) await requireRegularTree(source);
    else if (!entryMetadata.isFile()) usage(`unsupported package source type: ${source}`);
  }
}

const options = parseArgs(process.argv.slice(2));
const binary = path.resolve(options.binary);
const requestedOutput = path.resolve(options.output);
const requestedParent = path.dirname(requestedOutput);
const parentMetadata = await lstat(requestedParent).catch(() => null);
if (!parentMetadata?.isDirectory() || parentMetadata.isSymbolicLink()) {
  usage("output parent must already exist as a regular, non-symlink directory");
}
const outputRoot = path.join(await realpath(requestedParent), path.basename(requestedOutput));
await requireRegularFile(binary, "binary");
if (await lstat(outputRoot).catch(() => null)) usage("output directory must not already exist");

const methodsSource = path.join(root, "methods");
const guidesSource = path.join(root, "docs/guides");
const skillsSource = path.join(root, "skills");
const skillsInstaller = path.join(root, "scripts/install-skills.mjs");
if (isInside(outputRoot, methodsSource) || isInside(outputRoot, guidesSource) || isInside(outputRoot, skillsSource)) {
  usage("output directory must not be nested inside package source directories");
}
await requireRegularTree(methodsSource);
await requireRegularTree(skillsSource);
const skillEntries = await readdir(skillsSource, { withFileTypes: true });
if (skillEntries.some((entry) => !entry.isDirectory()) || JSON.stringify(skillEntries.map((entry) => entry.name).sort()) !== JSON.stringify([...expectedSkills].sort())) {
  usage("skills source must contain exactly the eight release skill directories");
}
await requireRegularFile(skillsInstaller, "scripts/install-skills.mjs");
for (const file of ["LICENSE", "README.md", "SECURITY.md", "THIRD_PARTY_NOTICES.md"]) {
  await requireRegularFile(path.join(root, file), file);
}
for (const file of ["HOST_AI.md", "INSTALLATION.md", "MIGRATIONS.md", "PRIVACY_AND_DELETION.md", "PROVIDERS.md", "RECOVERY.md"]) {
  await requireRegularFile(path.join(guidesSource, file), `docs/guides/${file}`);
}

const cargo = await readFile(path.join(root, "Cargo.toml"), "utf8");
const workspacePackage = cargo.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/)?.[1] ?? "";
const version = workspacePackage.match(/^version\s*=\s*"([^"]+)"$/m)?.[1];
if (!version) throw new Error("workspace version is missing");

await mkdir(outputRoot);
try {
const executableName = process.platform === "win32" ? "quisum.exe" : "quisum";
await mkdir(path.join(outputRoot, "bin"), { recursive: true });
await mkdir(path.join(outputRoot, "share/quisum"), { recursive: true });
await cp(binary, path.join(outputRoot, "bin", executableName));
if (process.platform !== "win32") await chmod(path.join(outputRoot, "bin", executableName), 0o755);
await cp(methodsSource, path.join(outputRoot, "share/quisum/methods"), {
  recursive: true,
  dereference: false,
  errorOnExist: true,
});
await cp(skillsSource, path.join(outputRoot, "share/quisum/skills"), {
  recursive: true,
  dereference: false,
  errorOnExist: true,
});
await mkdir(path.join(outputRoot, "share/quisum/scripts"));
await cp(skillsInstaller, path.join(outputRoot, "share/quisum/scripts/install-skills.mjs"));

for (const file of ["LICENSE", "README.md", "SECURITY.md", "THIRD_PARTY_NOTICES.md"]) {
  await cp(path.join(root, file), path.join(outputRoot, file));
}
await mkdir(path.join(outputRoot, "docs"));
for (const file of ["HOST_AI.md", "INSTALLATION.md", "MIGRATIONS.md", "PRIVACY_AND_DELETION.md", "PROVIDERS.md", "RECOVERY.md"]) {
  await cp(path.join(guidesSource, file), path.join(outputRoot, "docs", file));
}

const manifest = {
  schema_version: "1.0.0",
  product: "Quisum CLI",
  version,
  platform: options.platform,
  source_commit: process.env.GITHUB_SHA || null,
  unsigned: true,
  methods_directory: "share/quisum/methods",
  skills_directory: "share/quisum/skills",
  skills_installer: "share/quisum/scripts/install-skills.mjs",
};
await writeFile(path.join(outputRoot, "PACKAGE_MANIFEST.json"), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
} catch (error) {
  await rm(outputRoot, { recursive: true, force: true }).catch(() => {});
  throw error;
}
console.log(`Packaged Quisum CLI ${version} for ${options.platform} at ${outputRoot}.`);
