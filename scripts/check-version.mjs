import fs from "node:fs";

const packageJson = JSON.parse(fs.readFileSync("package.json", "utf8"));
const tauriConfig = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const cargo = fs.readFileSync("src-tauri/Cargo.toml", "utf8");

const cargoPackage = cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
const cargoVersion = cargoPackage?.[1];

const versions = {
  package: packageJson.version,
  tauri: tauriConfig.version,
  cargo: cargoVersion,
};

const unique = new Set(Object.values(versions));

if (unique.size !== 1 || unique.has(undefined)) {
  console.error("BOSCode version mismatch:", versions);
  process.exit(1);
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(packageJson.version)) {
  console.error("BOSCode version is not valid SemVer:", packageJson.version);
  process.exit(1);
}

console.log(`BOSCode version consistency OK: v${packageJson.version}`);
