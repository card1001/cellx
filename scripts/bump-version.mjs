#!/usr/bin/env node
// 버전을 한 번에 바꿉니다: node scripts/bump-version.mjs 0.2.0
// Cargo.toml(작업 공간), Cargo.lock, tauri.conf.json, package.json을 함께 고칩니다.
// MSI는 숫자 버전만 받으므로(주 버전 ≤ 255, 부 ≤ 255, 패치 ≤ 65535) 접미사는 쓰지 않습니다.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];
const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(version ?? "");
if (!m || +m[1] > 255 || +m[2] > 255 || +m[3] > 65535) {
  console.error("사용법: node scripts/bump-version.mjs <주.부.패치>  (예: 0.2.0, 숫자만)");
  process.exit(1);
}

const edit = (rel, fn) => {
  const p = join(root, rel);
  const before = readFileSync(p, "utf8");
  const after = fn(before);
  if (after === before) throw new Error(`${rel}: 버전 줄을 찾지 못했습니다`);
  writeFileSync(p, after);
  console.log(`  ${rel}`);
};

console.log(`버전 → ${version}`);
edit("Cargo.toml", (s) =>
  s.replace(/(\[workspace\.package\][^\[]*?\nversion = ")[^"]*(")/, `$1${version}$2`),
);
edit("Cargo.lock", (s) =>
  s.replace(/(\nname = "cellx[\w-]*"\nversion = ")[^"]*(")/g, `$1${version}$2`),
);
for (const rel of ["apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/package.json"]) {
  edit(rel, (s) => s.replace(/("version": ")[^"]*(")/, `$1${version}$2`));
}
console.log(`\n다음 순서로 배포합니다:
  git commit -am "v${version}"
  git tag v${version}
  git push && git push origin v${version}`);
