import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [version, dmgPath] = process.argv.slice(2);
if (!version || !dmgPath) {
  throw new Error("用法：node scripts/update-cask.mjs <version> <dmg-path>");
}

const caskPath = join(process.cwd(), "Casks", "lingdongdao.rb");
const sha256 = createHash("sha256").update(readFileSync(dmgPath)).digest("hex");
let source = readFileSync(caskPath, "utf8");
source = source.replace(/version "[^"]+"/, `version "${version.replace(/^v/, "")}"`);
source = source.replace(/sha256 (?:"[^"]+"|:no_check)/, `sha256 "${sha256}"`);
writeFileSync(caskPath, source);
console.log(`cask ${version} ${sha256}`);
