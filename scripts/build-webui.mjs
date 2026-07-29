import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src");
const out = join(root, "dist-webui");

const index = readFileSync(join(src, "index.html"), "utf8");
const styles = readFileSync(join(src, "styles.css"), "utf8");
const main = readFileSync(join(src, "main.js"), "utf8");
const compat = JSON.parse(readFileSync(join(src, "webui-compat.json"), "utf8"));

if (/<\/script/i.test(main) || /<!--/.test(main)) {
  throw new Error("main.js 含有无法安全内联的内容");
}
if (/<\/style/i.test(styles)) {
  throw new Error("styles.css 含有无法安全内联的内容");
}
if (!compat.minAppVersion) {
  throw new Error("webui-compat.json 缺少 minAppVersion");
}

const bootstrap = /[ \t]*<!-- webui-bootstrap-start[\s\S]*?<!-- webui-bootstrap-end -->/;
let html = index.replace(bootstrap, () => `    <script type="module">\n${main}\n</script>`);
html = html.replace('<link rel="stylesheet" href="styles.css" />', () => `<style>\n${styles}\n</style>`);

if (/webui-bootstrap|__lingdongdaoHotLoaded|document\.open\(\)/.test(html)) {
  throw new Error("热更新包中残留了引导逻辑");
}

let revision = "local";
try {
  revision = execFileSync("git", ["rev-parse", "--short=7", "HEAD"], {
    cwd: root,
    stdio: ["ignore", "pipe", "ignore"],
  })
    .toString()
    .trim();
} catch {}

const now = new Date();
const stamp = [
  now.getUTCFullYear(),
  String(now.getUTCMonth() + 1).padStart(2, "0"),
  String(now.getUTCDate()).padStart(2, "0"),
].join("") + "." + [
  String(now.getUTCHours()).padStart(2, "0"),
  String(now.getUTCMinutes()).padStart(2, "0"),
].join("");
const version = `${stamp}-${revision}`;
const sha256 = createHash("sha256").update(html, "utf8").digest("hex");

mkdirSync(out, { recursive: true });
writeFileSync(join(out, "webui.html"), html);
writeFileSync(
  join(out, "webui.json"),
  JSON.stringify({
    version,
    sha256,
    minAppVersion: compat.minAppVersion,
    ...(compat.maxAppVersion ? { maxAppVersion: compat.maxAppVersion } : {}),
  }, null, 2) + "\n",
);

console.log(`webui ${version}`);
console.log(`sha256 ${sha256}`);
