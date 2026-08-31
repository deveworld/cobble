import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createStoredZip,
  isDataPackZipFile,
  readStoredZipEntries,
  type ZipFileLike
} from "./datapackZip.ts";

test("data pack ZIP includes only pack, data, and asset files", async () => {
  const files: ZipFileLike[] = [
    {
      path: ".cobble/source_map.json",
      content: "{}"
    },
    {
      path: "assets/demo/models/item/test.json",
      content: '{"parent":"minecraft:item/generated"}'
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
      "assets/demo/models/item/test.json",
      "data/demo/function/load.mcfunction",
      "data/minecraft/tags/function/load.json",
      "pack.mcmeta"
    ]
  );

  const entries = readStoredZipEntries(
    new Uint8Array(await createStoredZip(zipFiles).arrayBuffer())
  );
  assert.deepEqual(
    entries.map((entry) => entry.name),
    [
      "assets/demo/models/item/test.json",
      "data/demo/function/load.mcfunction",
      "data/minecraft/tags/function/load.json",
      "pack.mcmeta"
    ]
  );
  assert.equal(entries[0].content, '{"parent":"minecraft:item/generated"}');
  assert.equal(entries[1].content, "say load\n");
  assert.equal(entries[2].content, '{"values":["demo:load"]}');
  assert.equal(entries[3].content, '{"pack":{"description":"test"}}');
});

test("data pack ZIP rejects unsafe entry paths", () => {
  const unsafePaths = [
    "data/../escape.mcfunction",
    "data/demo/../escape.mcfunction",
    "data//demo.mcfunction",
    "data/./demo.mcfunction",
    "assets/../escape.json",
    "/data/demo.mcfunction",
    "data\\demo.mcfunction",
    "data/demo:bad.mcfunction",
    "pack.mcmeta/extra"
  ];

  for (const path of unsafePaths) {
    assert.equal(isDataPackZipFile({ path, content: "" }), false, path);
    assert.throws(
      () => createStoredZip([{ path, content: "" }]),
      /Unsafe data pack ZIP path/,
      path
    );
  }
});
