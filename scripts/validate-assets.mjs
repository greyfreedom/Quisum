#!/usr/bin/env node

import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const errors = [];
const entityId = /^[a-z][a-z0-9_]{0,31}_[0-9A-HJKMNP-TV-Z]{26}$/;
const expectedMethods = new Set([
  "free-reflection",
  "event-debrief",
  "values-clarification",
  "decision-reflection",
  "pattern-validation",
  "behavioral-experiment",
  "periodic-review",
]);
const requiredMethodFiles = [
  "method.yaml",
  "facilitator.md",
  "output.schema.json",
  "memory-policy.yaml",
  "safety.md",
  "examples.jsonl",
  "evals/cases.jsonl",
];

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const absolute = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if ([".git", "target", "node_modules", "dist"].includes(entry.name)) {
        return [];
      }
      return walk(absolute);
    }
    return [absolute];
  }));
  return nested.flat();
}

function relative(file) {
  return path.relative(root, file);
}

function fail(file, message) {
  errors.push(`${relative(file)}: ${message}`);
}

function parseJson(text, file, label = "JSON") {
  try {
    return JSON.parse(text);
  } catch (error) {
    fail(file, `invalid ${label}: ${error.message}`);
    return null;
  }
}

function scalar(yaml, key) {
  const match = yaml.match(new RegExp(`^${key}:\\s*(.*?)\\s*$`, "m"));
  if (!match) return null;
  const value = match[1];
  if (value === "null") return null;
  return value.replace(/^"(.*)"$/, "$1");
}

function collectRefs(value, found = []) {
  if (Array.isArray(value)) {
    for (const item of value) collectRefs(item, found);
  } else if (value && typeof value === "object") {
    if (typeof value.$ref === "string") found.push(value.$ref);
    for (const child of Object.values(value)) collectRefs(child, found);
  }
  return found;
}

function checkFixtureIds(value, file, key = null) {
  if (Array.isArray(value)) {
    for (const item of value) checkFixtureIds(item, file, key);
    return;
  }
  if (!value || typeof value !== "object") return;

  for (const [childKey, child] of Object.entries(value)) {
    const isSingleReference = childKey === "id" ||
      childKey.endsWith("_id") ||
      ["supersedes"].includes(childKey);
    const isReferenceArray = [
      "supporting_evidence",
      "counterevidence",
      "related_claims",
    ].includes(childKey);

    if (isSingleReference && typeof child === "string" && !entityId.test(child)) {
      fail(file, `${childKey} is not a canonical entity ID: ${child}`);
    }
    if (isReferenceArray && Array.isArray(child)) {
      for (const id of child) {
        if (typeof id !== "string" || !entityId.test(id)) {
          fail(file, `${childKey} contains a non-canonical entity ID: ${String(id)}`);
        }
      }
    }
    checkFixtureIds(child, file, childKey);
  }
}

const files = await walk(root);
const jsonFiles = files.filter((file) => file.endsWith(".json"));
const jsonlFiles = files.filter((file) => file.endsWith(".jsonl"));
const markdownFiles = files.filter((file) => file.endsWith(".md"));

for (const file of jsonFiles) {
  const text = await readFile(file, "utf8");
  const value = parseJson(text, file);
  if (!value) continue;

  if (relative(file).startsWith("schemas/")) {
    if (value.$schema !== "https://json-schema.org/draft/2020-12/schema") {
      fail(file, "schema must use JSON Schema draft 2020-12");
    }
    if (typeof value.$id !== "string" || !value.$id.startsWith("https://quisum.org/")) {
      fail(file, "schema must have a stable quisum.org $id");
    }
  }

  for (const ref of collectRefs(value)) {
    const target = ref.split("#", 1)[0];
    if (!target || /^https?:/.test(target)) continue;
    const resolved = path.resolve(path.dirname(file), target);
    try {
      if (!(await stat(resolved)).isFile()) fail(file, `$ref target is not a file: ${ref}`);
    } catch {
      fail(file, `missing $ref target: ${ref}`);
    }
  }
}

const evalIds = new Set();
for (const file of jsonlFiles) {
  const text = await readFile(file, "utf8");
  const lines = text.split(/\r?\n/).filter((line) => line.trim().length > 0);
  if (lines.length === 0) {
    fail(file, "JSONL file is empty");
    continue;
  }

  for (const [index, line] of lines.entries()) {
    const value = parseJson(line, file, `JSONL line ${index + 1}`);
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      if (value !== null) fail(file, `line ${index + 1} must contain an object`);
      continue;
    }
    if (value.schema_version !== "1.0.0") {
      fail(file, `line ${index + 1} has unsupported schema_version`);
    }
    if (relative(file).includes("/evals/") || relative(file).startsWith("evals/")) {
      if (!value.id?.startsWith("eval_")) fail(file, `line ${index + 1} lacks an eval ID`);
      if (evalIds.has(value.id)) fail(file, `duplicate eval ID ${value.id}`);
      evalIds.add(value.id);
      if (!Array.isArray(value.assertions) || value.assertions.length === 0) {
        fail(file, `eval ${value.id} has no assertions`);
      }
    }
    if (relative(file).startsWith("fixtures/")) checkFixtureIds(value, file);
  }
}

const methodDirectories = new Set(
  files
    .filter((file) => relative(file).startsWith("methods/"))
    .map((file) => relative(file).split(path.sep)[1])
    .filter(Boolean),
);

for (const method of expectedMethods) {
  if (!methodDirectories.has(method)) {
    errors.push(`methods/${method}: missing official method package`);
    continue;
  }
  const directory = path.join(root, "methods", method);
  for (const required of requiredMethodFiles) {
    try {
      if (!(await stat(path.join(directory, required))).isFile()) {
        errors.push(`methods/${method}/${required}: expected a file`);
      }
    } catch {
      errors.push(`methods/${method}/${required}: missing required method asset`);
    }
  }

  const manifestFile = path.join(directory, "method.yaml");
  const manifest = await readFile(manifestFile, "utf8");
  const id = scalar(manifest, "id");
  const version = scalar(manifest, "version");
  if (id !== method) fail(manifestFile, `manifest ID ${String(id)} does not match directory`);
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(version ?? "")) {
    fail(manifestFile, "version must be semantic x.y.z");
  }
  if (scalar(manifest, "schema_version") !== "1.0.0") {
    fail(manifestFile, "schema_version must be 1.0.0");
  }

  const memoryFile = path.join(directory, "memory-policy.yaml");
  const memory = await readFile(memoryFile, "utf8");
  if (scalar(memory, "method") !== `${method}@${version}`) {
    fail(memoryFile, "method provenance does not match manifest");
  }
  if (!/^human_review_required:\s*true\s*$/m.test(memory)) {
    fail(memoryFile, "human_review_required must be true");
  }

  const exampleFile = path.join(directory, "examples.jsonl");
  const firstExample = parseJson((await readFile(exampleFile, "utf8")).trim().split(/\r?\n/)[0], exampleFile, "example");
  if (firstExample?.method !== `${method}@${version}`) {
    fail(exampleFile, "example method does not match manifest");
  }
  if (!firstExample?.safety || typeof firstExample.safety.allow_persistence !== "boolean") {
    fail(exampleFile, "example must include an explicit safety decision");
  }
}

for (const extra of methodDirectories) {
  if (!expectedMethods.has(extra)) {
    errors.push(`methods/${extra}: undeclared official method package`);
  }
}

for (const file of markdownFiles) {
  const text = await readFile(file, "utf8");
  const links = [...text.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)].map((match) => match[1]);
  for (const link of links) {
    if (/^(?:https?:|mailto:|#)/.test(link)) continue;
    const clean = link.replace(/^<|>$/g, "").split("#", 1)[0].replace(/:\d+$/, "");
    if (!clean) continue;
    const target = path.resolve(path.dirname(file), clean);
    try {
      await stat(target);
    } catch {
      fail(file, `broken relative link: ${link}`);
    }
  }
}

if (errors.length > 0) {
  for (const error of errors.sort()) console.error(`ERROR ${error}`);
  console.error(`\nAsset validation failed with ${errors.length} error(s).`);
  process.exitCode = 1;
} else {
  console.log(`Validated ${jsonFiles.length} JSON schemas/assets, ${jsonlFiles.length} JSONL files, ${expectedMethods.size} method packages, and ${evalIds.size} eval cases.`);
}
