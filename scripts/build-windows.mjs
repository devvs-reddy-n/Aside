// Build a Windows installer on either Windows itself or the owner's Mac.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, resolve, delimiter } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const target = "x86_64-pc-windows-msvc";
const env = { ...process.env };
delete env.CARGO_TARGET_DIR;
const dependencyDir = resolve(root, "src-tauri/target.noindex/windows-deps");
const runtime = resolve(dependencyDir, "vc_redist.x64.exe");
const runtimeHash = "843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c";
mkdirSync(dependencyDir, { recursive: true });
if (!existsSync(runtime)) {
  const response = await fetch("https://download.visualstudio.microsoft.com/download/pr/ebdab8e5-1d7b-4d9f-a11b-cbb1720c3b12/843068991DAAA1F73AD9F6239BCE4D0F6A07A51F18C37EA2A867E9BECA71295C/VC_redist.x64.exe");
  if (!response.ok) throw new Error(`Microsoft runtime download failed: ${response.status}`);
  writeFileSync(runtime, Buffer.from(await response.arrayBuffer()));
}
if (createHash("sha256").update(readFileSync(runtime)).digest("hex") !== runtimeHash) {
  throw new Error("Microsoft's runtime file differs from the verified build dependency. Review its signature and update the pinned hash before packaging.");
}
if (process.platform === "darwin") {
  const brew = existsSync("/opt/homebrew/bin/brew") ? "/opt/homebrew" : "/usr/local";
  env.PATH = [resolve(brew, "opt/llvm/bin"), resolve(brew, "opt/lld/bin"), env.PATH].join(delimiter);
  env.XWIN_CACHE_DIR = resolve(root, ".xwin-cache");
  for (const command of ["cargo-xwin", "clang-cl", "lld-link", "llvm-rc", "makensis"]) {
    if (spawnSync(command, ["--version"], { env, stdio: "ignore" }).error) {
      throw new Error(`Missing ${command}. Install llvm, lld and nsis with Homebrew, then cargo install --locked cargo-xwin and rustup target add ${target}.`);
    }
  }
} else if (process.platform !== "win32") {
  throw new Error("This build script supports macOS and Windows.");
}

const args = ["run", "tauri", "build", "--", "--target", target, "--bundles", "nsis"];
if (process.platform === "darwin") args.push("--runner", "cargo-xwin");
args.push("--", "--locked");
const build = spawnSync(process.platform === "win32" ? "npm.cmd" : "npm", args, {
  cwd: root, env, stdio: "inherit", shell: process.platform === "win32",
});
if (build.error) throw build.error;
if (build.status !== 0) process.exit(build.status ?? 1);

const bundles = resolve(root, "src-tauri/target.noindex", target, "release/bundle/nsis");
const version = JSON.parse(readFileSync(resolve(root, "src-tauri/tauri.conf.json"), "utf8")).version;
const installer = `Aside_${version}_x64-setup.exe`;
if (!existsSync(resolve(bundles, installer))) throw new Error(`Expected the current ${version} Windows installer.`);
const output = resolve(root, "release-windows");
mkdirSync(output, { recursive: true });
copyFileSync(resolve(bundles, installer), resolve(output, "Aside-Setup.exe"));
console.log(`Windows installer ready: ${resolve(output, "Aside-Setup.exe")}`);
