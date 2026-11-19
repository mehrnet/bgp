import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const WEB_ROOT = path.resolve(import.meta.dirname, "..");
const MAX_FILE_LINES = 180;
const MAX_FN_LINES = 55;
const MAX_HELPER_FN_LINES = 40;
const MAX_NESTING = 4;

const failures = [];

function walkFiles(dir, exts, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walkFiles(full, exts, out);
    else if (exts.some((ext) => entry.name.endsWith(ext))) out.push(full);
  }
  return out;
}

function getLine(sf, pos) {
  return sf.getLineAndCharacterOfPosition(pos).line + 1;
}

function checkMaxDepth(node, sf, rel, depth = 0) {
  let nextDepth = depth;
  if (
    ts.isIfStatement(node) ||
    ts.isForStatement(node) ||
    ts.isForOfStatement(node) ||
    ts.isForInStatement(node) ||
    ts.isWhileStatement(node) ||
    ts.isSwitchStatement(node) ||
    ts.isTryStatement(node)
  ) {
    nextDepth += 1;
    if (nextDepth > MAX_NESTING) {
      failures.push(`${rel}:${getLine(sf, node.getStart(sf))}: nesting depth ${nextDepth} > ${MAX_NESTING}`);
      return;
    }
  }
  ts.forEachChild(node, (child) => checkMaxDepth(child, sf, rel, nextDepth));
}

function checkAst(filePath, iconIds) {
  const rel = path.relative(WEB_ROOT, filePath);
  const text = fs.readFileSync(filePath, "utf8");
  const lineCount = text.split("\n").length;
  if (lineCount > MAX_FILE_LINES) {
    failures.push(`${rel}: ${lineCount} lines exceeds limit ${MAX_FILE_LINES}`);
  }
  if (/@ts-ignore|@ts-expect-error/.test(text)) {
    failures.push(`${rel}: forbidden TypeScript suppression directive`);
  }

  const kind = filePath.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(filePath, text, ts.ScriptTarget.Latest, true, kind);

  function visit(node) {
    if (node.kind === ts.SyntaxKind.AnyKeyword) {
      failures.push(`${rel}:${getLine(sf, node.getStart(sf))}: forbidden explicit 'any' type`);
    }
    if (ts.isJsxAttribute(node) && node.name.getText(sf) === "style") {
      failures.push(`${rel}:${getLine(sf, node.getStart(sf))}: inline JSX style attribute forbidden; use CSS classes`);
    }
    if (ts.isJsxSelfClosingElement(node) && node.tagName.getText(sf) === "Icon") {
      for (const attr of node.attributes.properties) {
        if (ts.isJsxAttribute(attr) && attr.name.getText(sf) === "id" && attr.initializer && ts.isStringLiteral(attr.initializer)) {
          const iconId = attr.initializer.text;
          if (!iconIds.has(iconId)) {
            failures.push(`${rel}:${getLine(sf, attr.getStart(sf))}: unknown icon id "${iconId}" not in public/icons.svg`);
          }
        }
      }
    }
    if (ts.isFunctionDeclaration(node) || ts.isArrowFunction(node) || ts.isFunctionExpression(node) || ts.isMethodDeclaration(node)) {
      if (node.body && ts.isBlock(node.body)) {
        const start = getLine(sf, node.getStart(sf));
        const end = getLine(sf, node.end);
        const span = end - start + 1;
        const limit = filePath.endsWith(".tsx") ? MAX_FN_LINES : MAX_HELPER_FN_LINES;
        if (span > limit) {
          const name = node.name?.getText(sf) ?? "anonymous";
          failures.push(`${rel}:${start}: function ${name} is ${span} lines (limit ${limit})`);
        }
      }
      checkMaxDepth(node.body ?? node, sf, rel);
    }
    ts.forEachChild(node, visit);
  }
  visit(sf);
}

function loadSvgIcons() {
  const svg = fs.readFileSync(path.join(WEB_ROOT, "public/icons.svg"), "utf8");
  const ids = new Set();
  for (const match of svg.matchAll(/<symbol[^>]*\bid="([^"]+)"/g)) {
    ids.add(match[1]);
  }
  return ids;
}

function checkAssets() {
  for (const reqFile of ["public/_headers", "public/favicon.svg", "public/icons.svg", "public/openapi.json", "wrangler.toml"]) {
    if (!fs.existsSync(path.join(WEB_ROOT, reqFile))) {
      failures.push(`missing required asset: ${reqFile}`);
    }
  }
  try {
    JSON.parse(fs.readFileSync(path.join(WEB_ROOT, "public/openapi.json"), "utf8"));
  } catch (err) {
    failures.push(`invalid public/openapi.json: ${err.message}`);
  }
}

const iconIds = loadSvgIcons();
checkAssets();
for (const file of [...walkFiles(path.join(WEB_ROOT, "src"), [".ts", ".tsx"]), ...walkFiles(path.join(WEB_ROOT, "test"), [".ts", ".tsx"])]) {
  checkAst(file, iconIds);
}

if (failures.length > 0) {
  console.error("UI quality check failed:\n" + failures.map((f) => `  - ${f}`).join("\n"));
  process.exit(1);
}
console.log("UI quality check passed.");
