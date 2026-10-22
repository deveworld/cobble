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
  ...(await filesIn(path.join(repoRoot, "docs", "src"), ".md"))
].map((file) => path.resolve(repoRoot, file));

const expectedStaticFiles = [
  "out/index.html",
  "out/try/index.html",
  "out/404.html",
  "out/wasm/cobble_web_wasm_bg.wasm",
  "out/cobble-workshop.jpg",
  "out/icon.svg",
  "out/docs/index.html",
  "out/docs/language/variables.html"
];
const checkWebExport = process.env.COBBLE_LINK_CHECK_WEB_EXPORT !== "0";

const errors = [];
const anchorCache = new Map();

for (const file of markdownFiles) {
  await checkMarkdownFile(file);
}

if (checkWebExport) {
  for (const expected of expectedStaticFiles) {
    const target = path.join(webRoot, expected);
    if (!existsSync(target)) {
      errors.push(`Missing web export file: web/${expected}`);
    }
  }
}

if (errors.length > 0) {
  console.error(errors.join("\n"));
  process.exit(1);
}

console.log(
  checkWebExport
    ? `Checked ${markdownFiles.length} markdown files and ${expectedStaticFiles.length} web export files.`
    : `Checked ${markdownFiles.length} markdown files.`
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

      const [fileTarget, anchor] = target.split("#");
      const resolved = fileTarget ? path.resolve(path.dirname(file), fileTarget) : file;
      const location = `${path.relative(repoRoot, file)}:${index + 1}`;
      if (!existsSync(resolved)) {
        errors.push(`${location}: missing link target ${target}`);
      } else if (anchor && resolved.endsWith(".md") && !(await anchorsOf(resolved)).has(anchor)) {
        errors.push(`${location}: missing heading for ${target}`);
      }
    }
  }
}

function normalizeLinkTarget(target) {
  return target.trim().replace(/^<|>$/g, "");
}

function shouldSkipLink(target) {
  return /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("//");
}

// Heading ids as mdBook and GitHub generate them: lowercase, punctuation
// dropped, spaces turned into hyphens.
async function anchorsOf(file) {
  if (!anchorCache.has(file)) {
    const anchors = new Set();
    let inCodeFence = false;
    for (const line of (await readFile(file, "utf8")).split(/\r?\n/)) {
      if (line.trimStart().startsWith("```")) {
        inCodeFence = !inCodeFence;
        continue;
      }
      const heading = !inCodeFence && /^#{1,6}\s+(.*)$/.exec(line);
      if (heading) {
        anchors.add(
          heading[1]
            .trim()
            .toLowerCase()
            .replace(/[^\p{L}\p{N}\s_-]/gu, "")
            .replace(/\s/g, "-")
        );
      }
    }
    anchorCache.set(file, anchors);
  }
  return anchorCache.get(file);
}

async function filesIn(directory, extension) {
  const entries = await readdir(directory);
  const files = [];
  for (const entry of entries) {
    const file = path.join(directory, entry);
    const metadata = await stat(file);
    if (metadata.isDirectory()) {
      files.push(...(await filesIn(file, extension)));
    } else if (metadata.isFile() && file.endsWith(extension)) {
      files.push(path.relative(repoRoot, file));
    }
  }
  return files.sort();
}
