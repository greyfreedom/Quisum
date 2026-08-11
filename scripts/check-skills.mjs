#!/usr/bin/env node

import { execFile } from "node:child_process";
import { cp, lstat, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

const execFileAsync = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const installer = path.join(root, "scripts", "install-skills.mjs");
const synchronizer = path.join(root, "scripts", "sync-skills.mjs");
const skillsRoot = path.join(root, "skills");
const expected = new Map([
  ["quisum-free-reflection", "method.md"],
  ["quisum-event-debrief", "method.md"],
  ["quisum-values-clarification", "method.md"],
  ["quisum-decision-reflection", "method.md"],
  ["quisum-pattern-validation", "method.md"],
  ["quisum-behavioral-experiment", "method.md"],
  ["quisum-periodic-review", "method.md"],
  ["quisum-profile-curation", "curation-contract.md"],
]);
const toolNames = [
  "quisum_search_memory",
  "quisum_get_event",
  "quisum_get_claim",
  "quisum_get_experiment",
  "quisum_build_context",
  "quisum_propose_event",
  "quisum_propose_claim",
  "quisum_propose_open_question",
  "quisum_propose_experiment",
  "quisum_propose_counterevidence",
];
const errors = [];

function fail(file, message) {
  errors.push(`${path.relative(root, file)}: ${message}`);
}

async function requireRegularTree(directory) {
  const metadata = await lstat(directory).catch(() => null);
  if (!metadata?.isDirectory() || metadata.isSymbolicLink()) {
    fail(directory, "must be a regular, non-symlink directory");
    return;
  }
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    const child = await lstat(file);
    if (child.isSymbolicLink()) fail(file, "symlinks are forbidden in installable skills");
    else if (child.isDirectory()) await requireRegularTree(file);
    else if (!child.isFile()) fail(file, "special files are forbidden in installable skills");
  }
}

async function inventory(directory, prefix = "") {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const relative = path.join(prefix, entry.name);
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await inventory(file, relative));
    else if (entry.isFile()) result.push(relative);
  }
  return result.sort();
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

function quotedYaml(yaml, key) {
  return yaml.match(new RegExp(`^  ${key}:\\s+"([^"]*)"\\s*$`, "m"))?.[1] ?? null;
}

for (const [name, reference] of expected) {
  const directory = path.join(skillsRoot, name);
  await requireRegularTree(directory);
  const skillFile = path.join(directory, "SKILL.md");
  const agentFile = path.join(directory, "agents", "openai.yaml");
  const referenceFile = path.join(directory, "references", reference);
  const skill = await readFile(skillFile, "utf8").catch(() => "");
  const agent = await readFile(agentFile, "utf8").catch(() => "");
  const generated = await readFile(referenceFile, "utf8").catch(() => "");
  const expectedEntries = [
    "SKILL.md",
    `agents${path.sep}`,
    path.join("agents", "openai.yaml"),
    `references${path.sep}`,
    path.join("references", reference),
  ].sort();
  if (JSON.stringify(await treeInventory(directory)) !== JSON.stringify(expectedEntries)) {
    fail(directory, "must contain only SKILL.md, agents/openai.yaml, and its declared reference");
  }

  const frontmatter = skill.match(/^---\n([\s\S]*?)\n---\n/);
  if (!frontmatter) {
    fail(skillFile, "missing YAML frontmatter");
  } else {
    const lines = frontmatter[1].split("\n");
    if (lines.length !== 2 || !lines[0].startsWith("name: ") || !lines[1].startsWith("description: ")) {
      fail(skillFile, "frontmatter must contain only name and description");
    }
    if (lines[0] !== `name: ${name}`) fail(skillFile, "name must match the directory");
    const description = lines[1]?.slice("description: ".length) ?? "";
    if (description.length < 25 || description.length > 1024 || !description.includes("Use when")) {
      fail(skillFile, "description must explain behavior and trigger conditions");
    }
  }

  for (const fragment of [
    `references/${reference}`,
    "unreviewed",
    "credible immediate danger",
    "make no Quisum read or write call",
    "Never use shell, filesystem, database",
    "CLI or Desktop",
    ...toolNames,
  ]) {
    if (!skill.includes(fragment)) fail(skillFile, `missing required boundary: ${fragment}`);
  }
  if (/\bTODO\b|\[TODO/i.test(skill)) fail(skillFile, "contains template placeholder text");
  if (reference === "method.md") {
    const method = name.slice("quisum-".length);
    const manifest = await readFile(path.join(root, "methods", method, "method.yaml"), "utf8").catch(() => "");
    const version = manifest.match(/^version:\s*([0-9]+\.[0-9]+\.[0-9]+)\s*$/m)?.[1] ?? null;
    if (!version || !skill.includes(`${method}@${version}`)) {
      fail(skillFile, "must name the current canonical method version");
    }
  }
  if (!generated.startsWith("<!-- Generated by scripts/sync-skills.mjs.")) {
    fail(referenceFile, "missing generated-source marker");
  }

  const displayName = quotedYaml(agent, "display_name");
  const shortDescription = quotedYaml(agent, "short_description");
  const defaultPrompt = quotedYaml(agent, "default_prompt");
  if (!displayName) fail(agentFile, "display_name must be a quoted string");
  if (!shortDescription || shortDescription.length < 25 || shortDescription.length > 64) {
    fail(agentFile, "short_description must be a quoted 25–64 character string");
  }
  if (!defaultPrompt?.includes(`$${name}`)) {
    fail(agentFile, "default_prompt must explicitly invoke the skill by name");
  }
  const agentLines = agent.trimEnd().split("\n");
  if (agentLines.length !== 4 || agentLines[0] !== "interface:") {
    fail(agentFile, "must contain only the three declared interface fields");
  }
}

const skillRootEntries = await readdir(skillsRoot, { withFileTypes: true });
const visibleSkillDirectories = skillRootEntries.map((entry) => entry.name).sort();
const expectedDirectories = [...expected.keys()].sort();
if (skillRootEntries.some((entry) => !entry.isDirectory()) || JSON.stringify(visibleSkillDirectories) !== JSON.stringify(expectedDirectories)) {
  errors.push(`skills: expected exactly ${expectedDirectories.join(", ")}`);
}

try {
  await execFileAsync(process.execPath, [synchronizer, "--check"], { cwd: root });
} catch (error) {
  errors.push(`scripts/sync-skills.mjs: ${error.stderr || error.message}`.trim());
}

const temporary = await mkdtemp(path.join(os.tmpdir(), "quisum-skills-check-"));
try {
  for (const host of ["codex", "claude-code"]) {
    const destination = path.join(temporary, host, "skills");
    await mkdir(destination, { recursive: true });
    await execFileAsync(process.execPath, [installer, "--host", host, "--destination", destination], { cwd: root });
    for (const name of expected.keys()) {
      const source = path.join(skillsRoot, name);
      const installed = path.join(destination, name);
      const metadata = await lstat(path.join(installed, "SKILL.md")).catch(() => null);
      if (!metadata?.isFile() || metadata.isSymbolicLink()) {
        errors.push(`installer ${host}: missing regular ${name}/SKILL.md`);
      }
      const sourceFiles = await inventory(source);
      const installedFiles = await inventory(installed).catch(() => []);
      if (JSON.stringify(installedFiles) !== JSON.stringify(sourceFiles)) {
        errors.push(`installer ${host}: ${name} file inventory differs from the source`);
        continue;
      }
      for (const file of sourceFiles) {
        const sourceBytes = await readFile(path.join(source, file));
        const installedBytes = await readFile(path.join(installed, file));
        if (!sourceBytes.equals(installedBytes)) {
          errors.push(`installer ${host}: ${name}/${file} differs from the source`);
        }
      }
    }
  }

  const collision = path.join(temporary, "collision", "skills");
  const first = expected.keys().next().value;
  await mkdir(path.join(collision, first), { recursive: true });
  await writeFile(path.join(collision, first, "owner-marker"), "pre-existing\n", "utf8");
  let rejected = false;
  try {
    await execFileAsync(process.execPath, [installer, "--host", "codex", "--destination", collision], { cwd: root });
  } catch {
    rejected = true;
  }
  if (!rejected) errors.push("installer collision test: existing target was not rejected");
  const collisionEntries = (await readdir(collision)).sort();
  if (JSON.stringify(collisionEntries) !== JSON.stringify([first])) {
    errors.push("installer collision test: partial installation was left behind");
  }
  const marker = await readFile(path.join(collision, first, "owner-marker"), "utf8").catch(() => "");
  if (marker !== "pre-existing\n") errors.push("installer collision test: existing content changed");

  const wrongName = path.join(temporary, "not-a-skills-directory");
  await mkdir(wrongName);
  let wrongNameRejected = false;
  try {
    await execFileAsync(process.execPath, [installer, "--host", "codex", "--destination", wrongName], { cwd: root });
  } catch {
    wrongNameRejected = true;
  }
  if (!wrongNameRejected || (await readdir(wrongName)).length !== 0) {
    errors.push("installer destination-name test: unsafe destination was accepted or changed");
  }

  const tamperedRoot = path.join(temporary, "tampered-package");
  const tamperedInstaller = path.join(tamperedRoot, "scripts", "install-skills.mjs");
  await mkdir(path.dirname(tamperedInstaller), { recursive: true });
  await cp(installer, tamperedInstaller);
  await cp(skillsRoot, path.join(tamperedRoot, "skills"), { recursive: true });
  await writeFile(
    path.join(tamperedRoot, "skills", first, "unexpected-executable.sh"),
    "#!/bin/sh\nexit 1\n",
    "utf8",
  );
  const tamperedDestination = path.join(temporary, "tampered-destination", "skills");
  await mkdir(tamperedDestination, { recursive: true });
  let tamperedRejected = false;
  try {
    await execFileAsync(process.execPath, [tamperedInstaller, "--host", "codex", "--destination", tamperedDestination]);
  } catch {
    tamperedRejected = true;
  }
  if (!tamperedRejected || (await readdir(tamperedDestination)).length !== 0) {
    errors.push("installer source-layout test: undeclared executable asset was accepted or partially installed");
  }
} finally {
  await rm(temporary, { recursive: true, force: true });
}

if (errors.length > 0) {
  for (const error of errors.sort()) console.error(`ERROR ${error}`);
  console.error(`\nSkill validation failed with ${errors.length} error(s).`);
  process.exitCode = 1;
} else {
  console.log(`Validated ${expected.size} installable Quisum skills, synchronized references, and both host installation paths.`);
}
