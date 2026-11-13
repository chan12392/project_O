import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const packages = JSON.parse(readFileSync(`${root}app/package-lock.json`, "utf8")).packages;
const notices = readFileSync(`${root}THIRD_PARTY_LICENSES.txt`, "utf8");
const separator = /^={78}\r?$/m;
const blocks = notices.split(separator);
const missing = [];
const listed = new Set();

for (const line of notices.match(/^npm .*$/gm) ?? []) {
  const match = /^npm (\S+) (\S+) \(([^;\r\n]+); [^\r\n]+\)\r?$/.exec(line);
  if (!match) {
    missing.push(`Malformed entry: ${line}`);
    continue;
  }
  const [, path, version, license] = match;
  const pkg = packages[path];
  if (!pkg || pkg.dev || pkg.version !== version || pkg.license !== license || listed.has(path)) {
    missing.push(`Unexpected or duplicate entry: ${path}@${version}`);
  }
  listed.add(path);
}

for (const [path, pkg] of Object.entries(packages)) {
  if (!path || pkg.dev) continue;
  const prefix = `npm ${path} ${pkg.version} (${pkg.license}; `;
  const index = blocks.findIndex((block, i) =>
    i % 2 === 1 && block.split(/\r?\n/).some(line => line.startsWith(prefix) && line.endsWith(")"))
  );
  if (!pkg.version || !pkg.license || index < 0 || !blocks[index + 1]?.trim()) {
    missing.push(`${path}@${pkg.version ?? "?"} (${pkg.license ?? "no license"})`);
  }
}

if (missing.length) {
  console.error(`Third-party license mismatch:\n${missing.join("\n")}`);
  process.exitCode = 1;
} else {
  console.log("All npm runtime packages have third-party license entries.");
}
