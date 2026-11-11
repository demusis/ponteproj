"use strict";
// Every file the extension loads must be in the three lists that ship it:
// the Chrome Web Store package, the installer and the local install script.
// A file missing from a list ships a broken extension (the worker's
// importScripts fails as a whole), so the lists are checked against the code.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..", "..");
const read = relative => fs.readFileSync(path.join(root, relative), "utf8");

function loadedFiles() {
  const names = new Set(["manifest.json"]);
  const manifest = JSON.parse(read("extension/manifest.json"));
  for (const script of manifest.content_scripts || []) for (const file of script.js || []) names.add(file);
  if (manifest.background && manifest.background.service_worker) names.add(manifest.background.service_worker);
  if (manifest.action && manifest.action.default_popup) names.add(manifest.action.default_popup);
  for (const icon of Object.values(manifest.icons || {})) names.add(icon);
  for (const icon of Object.values((manifest.action && manifest.action.default_icon) || {})) names.add(icon);
  // Scripts the worker imports and scripts it registers at run time.
  const worker = read("extension/background.js");
  for (const call of worker.matchAll(/importScripts\(([^)]*)\)/g)) {
    for (const name of call[1].matchAll(/["']([^"']+)["']/g)) names.add(name[1]);
  }
  for (const name of worker.matchAll(/["']([A-Za-z0-9_-]+\.js)["']/g)) names.add(name[1]);
  // What the popup page pulls in.
  const popup = read("extension/popup.html");
  for (const ref of popup.matchAll(/(?:src|href)="([^"#?:]+\.(?:js|css))"/g)) names.add(ref[1]);
  // Every script that exists in the folder is expected to ship.
  for (const name of fs.readdirSync(path.join(root, "extension"))) if (name.endsWith(".js")) names.add(name);
  return [...names].sort();
}

test("every file the extension loads is in the store package, the installer and the install script", () => {
  const files = loadedFiles();
  assert.ok(files.includes("whatsapp-core.js") && files.includes("background.js"), "the scan sees the scripts");
  const lists = {
    "scripts/build-webstore-package.ps1": name => `'${name}'`,
    "scripts/install-app.ps1": name => `'${name}'`,
    "installer/Ponte.iss": name => `\\extension\\${name.replace(/\//g, "\\")}"`
  };
  for (const [list, needle] of Object.entries(lists)) {
    const text = read(list);
    for (const name of files) {
      // The store package script reads the manifest by itself.
      if (name === "manifest.json" && list === "scripts/build-webstore-package.ps1") continue;
      assert.ok(text.includes(needle(name)), `${name} is missing from ${list}`);
    }
  }
});
