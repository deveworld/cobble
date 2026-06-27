#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const writeMode = process.argv.includes("--write");

function read(relativePath) {
  return fs.readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function requiredMatch(label, content, regex) {
  const match = content.match(regex);
  if (!match) {
    throw new Error(`Could not find ${label}`);
  }
  return match[1];
}

const cargoToml = read("Cargo.toml");
const packFormatRs = read("src/pack_format.rs");

const cargoVersion = requiredMatch(
  "Cargo package version",
  cargoToml,
  /^version\s*=\s*"([^"]+)"/m
);
const minecraftVersion = requiredMatch(
  "SUPPORTED_MINECRAFT_VERSION",
  packFormatRs,
  /SUPPORTED_MINECRAFT_VERSION:\s*&str\s*=\s*"([^"]+)"/
);
const decimalPackFormat = packFormatRs.match(
  /SUPPORTED_PACK_FORMAT:\s*PackFormat\s*=\s*PackFormat::Decimal\((\d+),\s*(\d+)\)/
);
const integerPackFormat = packFormatRs.match(
  /SUPPORTED_PACK_FORMAT:\s*PackFormat\s*=\s*PackFormat::Integer\((\d+)\)/
);
const packFormat = decimalPackFormat
  ? `${decimalPackFormat[1]}.${decimalPackFormat[2]}`
  : integerPackFormat?.[1];

if (!packFormat) {
  throw new Error("Could not find SUPPORTED_PACK_FORMAT");
}

const generatedWebMetadata = `export const COBBLE_VERSION = ${JSON.stringify(cargoVersion)};\nexport const SUPPORTED_MINECRAFT_VERSION = ${JSON.stringify(minecraftVersion)};\nexport const SUPPORTED_PACK_FORMAT = ${JSON.stringify(packFormat)};\n`;

if (writeMode) {
  fs.writeFileSync(
    path.join(repoRoot, "web/lib/compilerMetadata.ts"),
    generatedWebMetadata
  );
  console.log(
    `wrote web metadata from Cargo/Rust constants: ${cargoVersion}, Minecraft ${minecraftVersion}, pack ${packFormat}`
  );
  process.exit(0);
}

const webMetadata = read("web/lib/compilerMetadata.ts");

const webVersion = requiredMatch(
  "web COBBLE_VERSION",
  webMetadata,
  /COBBLE_VERSION\s*=\s*"([^"]+)"/
);
const webMinecraftVersion = requiredMatch(
  "web SUPPORTED_MINECRAFT_VERSION",
  webMetadata,
  /SUPPORTED_MINECRAFT_VERSION\s*=\s*"([^"]+)"/
);
const webPackFormat = requiredMatch(
  "web SUPPORTED_PACK_FORMAT",
  webMetadata,
  /SUPPORTED_PACK_FORMAT\s*=\s*"([^"]+)"/
);

const mismatches = [
  ["COBBLE_VERSION", cargoVersion, webVersion],
  ["SUPPORTED_MINECRAFT_VERSION", minecraftVersion, webMinecraftVersion],
  ["SUPPORTED_PACK_FORMAT", packFormat, webPackFormat],
].filter(([, expected, actual]) => expected !== actual);

if (mismatches.length > 0) {
  for (const [name, expected, actual] of mismatches) {
    console.error(`${name} drift: Rust/Cargo=${expected}, web=${actual}`);
  }
  process.exit(1);
}

console.log(
  `web metadata matches Cargo/Rust constants: ${cargoVersion}, Minecraft ${minecraftVersion}, pack ${packFormat}`
);
