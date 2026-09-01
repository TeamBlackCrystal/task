import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const packageRoot = path.resolve(__dirname, "../..");
const compiledEntry = path.join(packageRoot, "dist", "index.js");
const srcRoot = path.join(packageRoot, "src");
const tsconfigPath = path.join(packageRoot, "tsconfig.json");

function newestMtimeMs(targetPath: string): number {
  const stat = fs.statSync(targetPath);
  if (!stat.isDirectory()) {
    return stat.mtimeMs;
  }

  let newest = 0;
  for (const entry of fs.readdirSync(targetPath, { withFileTypes: true })) {
    newest = Math.max(newest, newestMtimeMs(path.join(targetPath, entry.name)));
  }
  return newest;
}

function needsRebuild(): boolean {
  if (!fs.existsSync(compiledEntry)) {
    return true;
  }

  const distMtime = fs.statSync(compiledEntry).mtimeMs;
  const srcMtime = newestMtimeMs(srcRoot);
  const tsconfigMtime = fs.existsSync(tsconfigPath)
    ? fs.statSync(tsconfigPath).mtimeMs
    : 0;

  return srcMtime > distMtime || tsconfigMtime > distMtime;
}

export default function globalSetup() {
  if (!needsRebuild()) {
    return;
  }
  execSync("pnpm run build", { cwd: packageRoot, stdio: "inherit" });
}
