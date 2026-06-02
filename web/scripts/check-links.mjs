import { existsSync } from "node:fs";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const webRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(webRoot, "..");

const markdownFiles = [
  "README.md",
  "CHANGELOG.md",
  "PLAN.md",
  ...(await filesIn(path.join(repoRoot, "docs"), ".md"))
].map((file) => path.resolve(repoRoot, file));

const expectedStaticFiles = [
  "out/index.html",
  "out/try/index.html",
  "out/404.html",
  "out/wasm/cobble_web_wasm_bg.wasm",
  "out/cobble-workshop.jpg",
  "out/icon.svg"
];

const errors = [];

for (const file of markdownFiles) {
  await checkMarkdownFile(file);
}

for (const expected of expectedStaticFiles) {
  const target = path.join(webRoot, expected);
  if (!existsSync(target)) {
    errors.push(`Missing web export file: web/${expected}`);
  }
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}

console.log(
  `Checked ${markdownFiles.length} markdown files and ${expectedStaticFiles.length} web export files.`
);

async function checkMarkdownFile(file) {
  const content = await readFile(file, "utf8");
  let inCodeFence = false;

  for (const [index, line] of content.split(/\r?\n/).entries()) {
    if (line.trimStart().startsWith("```")) {
      inCodeFence = !inCodeFence;
      continue;
    }
    if (inCodeFence) {
      continue;
    }

    const matches = line.matchAll(/!?\[[^\]]*]\(([^)]+)\)/g);
    for (const match of matches) {
      const target = normalizeLinkTarget(match[1]);
      if (!target || shouldSkipLink(target)) {
        continue;
      }

      const [fileTarget] = target.split("#");
      if (!fileTarget) {
        continue;
      }
      const resolved = path.resolve(path.dirname(file), fileTarget);
      if (!existsSync(resolved)) {
        errors.push(`${path.relative(repoRoot, file)}:${index + 1}: missing link target ${target}`);
      }
    }
  }
}

function normalizeLinkTarget(target) {
  return target.trim().replace(/^<|>$/g, "");
}

function shouldSkipLink(target) {
  return (
    target.startsWith("#") ||
    /^[a-z][a-z0-9+.-]*:/i.test(target) ||
    target.startsWith("//")
  );
}

async function filesIn(directory, extension) {
  const entries = await readdir(directory);
  const files = [];
  for (const entry of entries) {
    const file = path.join(directory, entry);
    const metadata = await stat(file);
    if (metadata.isFile() && file.endsWith(extension)) {
      files.push(path.relative(repoRoot, file));
    }
  }
  return files.sort();
}
