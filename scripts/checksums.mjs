#!/usr/bin/env node

import { createHash, randomUUID } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, readdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

function usage(message) {
  if (message) console.error(`ERROR ${message}`);
  console.error("Usage: node scripts/checksums.mjs --root <artifact-directory> --output <checksum-file>");
  process.exit(2);
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!["--root", "--output"].includes(key) || !value || value.startsWith("--")) usage(`invalid argument ${key ?? ""}`);
    options[key.slice(2)] = value;
  }
  if (!options.root || !options.output) usage("both --root and --output are required");
  return options;
}

async function walk(directory, output, files = []) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const absolute = path.join(directory, entry.name);
    if (path.resolve(absolute) === output || entry.name === ".DS_Store") continue;
    const metadata = await lstat(absolute);
    if (metadata.isSymbolicLink()) throw new Error(`refusing symlinked artifact ${absolute}`);
    if (metadata.isDirectory()) await walk(absolute, output, files);
    else if (metadata.isFile()) files.push(absolute);
    else throw new Error(`unsupported artifact type ${absolute}`);
  }
  return files;
}

async function digest(file) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest("hex");
}

const options = parseArgs(process.argv.slice(2));
const root = path.resolve(options.root);
const output = path.resolve(options.output);
const metadata = await lstat(root).catch(() => null);
if (!metadata?.isDirectory() || metadata.isSymbolicLink()) usage("--root must be a regular directory");
if (path.dirname(output) !== root) usage("--output must be a direct child of --root");

const files = (await walk(root, output)).sort();
if (files.length === 0) usage("artifact directory contains no files");
const lines = [];
for (const file of files) {
  const relative = path.relative(root, file).split(path.sep).join("/");
  if (/\r|\n/.test(relative)) throw new Error("artifact names must not contain newlines");
  lines.push(`${await digest(file)}  ${relative}`);
}

const temporary = path.join(root, `.${path.basename(output)}.${process.pid}.${randomUUID()}.tmp`);
await writeFile(temporary, `${lines.join("\n")}\n`, { encoding: "utf8", mode: 0o644, flag: "wx" });
await rename(temporary, output);
console.log(`Wrote ${lines.length} SHA-256 checksums to ${output}.`);
