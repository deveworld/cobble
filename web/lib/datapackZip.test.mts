import assert from "node:assert/strict";
import { test } from "node:test";

import { createStoredZip, isDataPackZipFile, type ZipFileLike } from "./datapackZip.ts";

test("data pack ZIP includes only pack and data files", async () => {
  const files: ZipFileLike[] = [
    {
      path: ".cobble/source_map.json",
      content: "{}"
    },
    {
      path: "data/demo/function/load.mcfunction",
      content: "say load\n"
    },
    {
      path: "pack.mcmeta",
      content: '{"pack":{"description":"test"}}'
    },
    {
      path: "data/minecraft/tags/function/load.json",
      content: '{"values":["demo:load"]}'
    }
  ];

  const zipFiles = files.filter(isDataPackZipFile);
  assert.deepEqual(
    zipFiles.map((file) => file.path).sort(),
    [
      "data/demo/function/load.mcfunction",
      "data/minecraft/tags/function/load.json",
      "pack.mcmeta"
    ]
  );

  const entries = await readStoredZipEntries(createStoredZip(zipFiles));
  assert.deepEqual(
    entries.map((entry) => entry.name),
    [
      "data/demo/function/load.mcfunction",
      "data/minecraft/tags/function/load.json",
      "pack.mcmeta"
    ]
  );
  assert.equal(entries[0].content, "say load\n");
  assert.equal(entries[1].content, '{"values":["demo:load"]}');
  assert.equal(entries[2].content, '{"pack":{"description":"test"}}');
});

async function readStoredZipEntries(zip: Blob) {
  const bytes = new Uint8Array(await zip.arrayBuffer());
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const decoder = new TextDecoder();
  const entries: Array<{ name: string; content: string }> = [];
  let offset = 0;

  while (offset < bytes.length && view.getUint32(offset, true) === 0x04034b50) {
    const compressedSize = view.getUint32(offset + 18, true);
    const nameLength = view.getUint16(offset + 26, true);
    const extraLength = view.getUint16(offset + 28, true);
    const nameStart = offset + 30;
    const dataStart = nameStart + nameLength + extraLength;
    const dataEnd = dataStart + compressedSize;

    entries.push({
      name: decoder.decode(bytes.slice(nameStart, nameStart + nameLength)),
      content: decoder.decode(bytes.slice(dataStart, dataEnd))
    });

    offset = dataEnd;
  }

  assert.equal(view.getUint32(offset, true), 0x02014b50);
  return entries;
}
