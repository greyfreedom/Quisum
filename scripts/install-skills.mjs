#!/usr/bin/env node

import { constants } from "node:fs";
import { chmod, copyFile, lstat, mkdir, readdir, realpath, rm } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const skillsRoot = path.join(root, "skills");
const skillReferences = new Map([
  ["quisum-free-reflection", "method.md"],
  ["quisum-event-debrief", "method.md"],
  ["quisum-values-clarification", "method.md"],
  ["quisum-decision-reflection", "method.md"],
  ["quisum-pattern-validation", "method.md"],
  ["quisum-behavioral-experiment", "method.md"],
  ["quisum-periodic-review", "method.md"],
  ["quisum-profile-curation", "curation-contract.md"],
]);
const skills = [...skillReferences.keys()];

function usage(message) {
  if (message) console.error(`ERROR ${message}`);
  console.error("Usage: node scripts/install-skills.mjs --host <codex|claude-code> --destination <absolute-skills-directory>");
  process.exit(2);
}

function parseArgs(argv) {
  if (argv.length !== 4) usage("exactly two options are required");
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!new Set(["--host", "--destination"]).has(key) || !value || value.startsWith("--")) {
      usage(`invalid argument ${key ?? ""}`);
    }
    const name = key.slice(2);
    if (options[name]) usage(`duplicate option ${key}`);
    options[name] = value;
  }
  if (!new Set(["codex", "claude-code"]).has(options.host)) usage("host must be codex or claude-code");
  if (!path.isAbsolute(options.destination)) usage("destination must be an absolute path");
  return options;
}

function isInside(candidate, directory) {
  const relative = path.relative(directory, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

async function validateTree(directory) {
  const metadata = await lstat(directory).catch(() => null);
  if (!metadata?.isDirectory() || metadata.isSymbolicLink()) {
    throw new Error(`${directory} must be a regular, non-symlink directory`);
  }
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (/\r|\n|\0/.test(entry.name)) throw new Error("skill paths must not contain control characters");
    const source = path.join(directory, entry.name);
    const child = await lstat(source);
    if (child.isSymbolicLink()) throw new Error(`skill source must not contain symlinks: ${source}`);
    if (child.isDirectory()) await validateTree(source);
    else if (!child.isFile()) throw new Error(`skill source contains an unsupported file type: ${source}`);
  }
}

async function copyTreeContents(source, destination) {
  const metadata = await lstat(source);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) throw new Error(`invalid source directory: ${source}`);
  for (const entry of await readdir(source, { withFileTypes: true })) {
    const from = path.join(source, entry.name);
    const to = path.join(destination, entry.name);
    const child = await lstat(from);
    if (child.isSymbolicLink()) throw new Error(`refusing symlink: ${from}`);
    if (child.isDirectory()) {
      await mkdir(to, { mode: child.mode & 0o777 });
      await copyTreeContents(from, to);
    } else if (child.isFile()) {
      await copyFile(from, to, constants.COPYFILE_EXCL);
      await chmod(to, child.mode & 0o777);
    } else {
      throw new Error(`refusing special file: ${from}`);
    }
  }
}

async function treeInventory(directory, prefix = "") {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name);
    if (entry.isDirectory()) {
      result.push(`${relative}${path.sep}`);
      result.push(...await treeInventory(path.join(directory, entry.name), relative));
    } else {
      result.push(relative);
    }
  }
  return result.sort();
}

const options = parseArgs(process.argv.slice(2));
if (/\r|\n|\0/.test(options.destination)) usage("destination must not contain control characters");
const destinationMetadata = await lstat(options.destination).catch(() => null);
if (!destinationMetadata?.isDirectory() || destinationMetadata.isSymbolicLink()) {
  usage("destination must already exist as a regular, non-symlink directory");
}
const destination = await realpath(options.destination);
if (destination === path.parse(destination).root || path.basename(destination) !== "skills") {
  usage("destination must be a dedicated directory named skills, not a filesystem root");
}
if (isInside(destination, skillsRoot)) usage("destination must not be inside the packaged skill source");

const packagedEntries = await readdir(skillsRoot, { withFileTypes: true });
const packagedNames = packagedEntries.map((entry) => entry.name).sort();
if (packagedEntries.some((entry) => !entry.isDirectory()) || JSON.stringify(packagedNames) !== JSON.stringify([...skills].sort())) {
  usage("packaged skill source must contain exactly the declared eight skill directories");
}

for (const skill of skills) {
  const source = path.join(skillsRoot, skill);
  await validateTree(source);
  const expectedEntries = [
    "SKILL.md",
    `agents${path.sep}`,
    path.join("agents", "openai.yaml"),
    `references${path.sep}`,
    path.join("references", skillReferences.get(skill)),
  ].sort();
  if (JSON.stringify(await treeInventory(source)) !== JSON.stringify(expectedEntries)) {
    usage(`skill source contains undeclared files or directories: ${skill}`);
  }
  if (await lstat(path.join(destination, skill)).catch(() => null)) {
    usage(`refusing to overwrite existing skill: ${path.join(destination, skill)}`);
  }
}

const installed = [];
try {
  for (const skill of skills) {
    const target = path.join(destination, skill);
    const source = path.join(skillsRoot, skill);
    const sourceMetadata = await lstat(source);
    await mkdir(target, { mode: sourceMetadata.mode & 0o777 });
    installed.push(target);
    await copyTreeContents(source, target);
  }
} catch (error) {
  for (const target of installed.reverse()) {
    await rm(target, { recursive: true, force: true }).catch(() => {});
  }
  throw error;
}

console.log(`Installed ${skills.length} Quisum skills for ${options.host} into ${destination}.`);
