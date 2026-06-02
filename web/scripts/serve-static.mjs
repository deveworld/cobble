import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(process.argv[2] ?? "out");
const port = Number(process.argv[3] ?? "4173");
const basePath = normalizeBasePath(process.argv[4] ?? "");

const contentTypes = new Map([
  [".css", "text/css; charset=utf-8"],
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".mcmeta", "application/json; charset=utf-8"],
  [".svg", "image/svg+xml"],
  [".wasm", "application/wasm"],
  [".jpg", "image/jpeg"],
  [".jpeg", "image/jpeg"],
  [".png", "image/png"],
  [".txt", "text/plain; charset=utf-8"]
]);

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://127.0.0.1");
  let pathname = decodeURIComponent(url.pathname);

  if (basePath) {
    if (pathname === basePath) {
      response.writeHead(308, { location: `${basePath}/` });
      response.end();
      return;
    }
    if (!pathname.startsWith(`${basePath}/`)) {
      response.writeHead(404);
      response.end("Not found");
      return;
    }
    pathname = pathname.slice(basePath.length) || "/";
  }

  const file = await resolveStaticFile(pathname);
  if (!file) {
    response.writeHead(404);
    response.end("Not found");
    return;
  }

  response.writeHead(200, {
    "content-type": contentTypes.get(path.extname(file)) ?? "application/octet-stream"
  });
  createReadStream(file).pipe(response);
});

server.listen(port, "127.0.0.1", () => {
  const script = fileURLToPath(import.meta.url);
  console.log(`Serving ${root} from ${script} at http://127.0.0.1:${port}${basePath}/`);
});

async function resolveStaticFile(pathname) {
  const normalized = path.normalize(pathname).replace(/^(\.\.[/\\])+/, "");
  let candidate = path.join(root, normalized);
  if (!candidate.startsWith(root)) {
    return null;
  }

  const metadata = await safeStat(candidate);
  if (metadata?.isDirectory()) {
    candidate = path.join(candidate, "index.html");
  } else if (!metadata && !path.extname(candidate)) {
    candidate = path.join(candidate, "index.html");
  }

  const finalMetadata = await safeStat(candidate);
  return finalMetadata?.isFile() ? candidate : null;
}

async function safeStat(file) {
  try {
    return await stat(file);
  } catch {
    return null;
  }
}

function normalizeBasePath(value) {
  if (!value || value === "/") {
    return "";
  }
  const withLeadingSlash = value.startsWith("/") ? value : `/${value}`;
  return withLeadingSlash.replace(/\/+$/, "");
}
