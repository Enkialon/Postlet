/**
 * 扫描 src 下全部 .vue 模板中文，合并进 locales JSON：
 * - 已有 key 不覆盖
 * - 新 key 追加（zh = 原文；en = 原文占位，人工再译）
 *
 * 用法：node plugins/vite-plugin-auto-i18n/extract.mjs
 */
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse as parseSfc } from '@vue/compiler-sfc';
import { NodeTypes, baseParse } from '@vue/compiler-dom';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '../..');
const SRC = join(ROOT, 'src');
const LOCALES = join(SRC, 'i18n', 'locales');

const HAS_CJK = /[\u4e00-\u9fff]/
const I18N_ATTRS = new Set([
  'title',
  'placeholder',
  'aria-label',
  'alt',
  'aria-placeholder',
  'aria-description',
]);

function walkDir(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walkDir(p, out);
    else if (name.endsWith('.vue')) out.push(p);
  }
  return out;
}

function walk(nodes, skip, keys) {
  for (const node of nodes) {
    if (node.type === NodeTypes.ELEMENT) {
      const no =
        skip ||
        node.props.some(
          (p) =>
            (p.type === NodeTypes.DIRECTIVE && p.name === 'no-i18n') ||
            (p.type === NodeTypes.ATTRIBUTE && p.name === 'v-no-i18n'),
        );
      if (!no) {
        for (const prop of node.props) {
          if (prop.type !== NodeTypes.ATTRIBUTE || !prop.value) continue;
          if (!I18N_ATTRS.has(prop.name)) continue;
          const c = prop.value.content.trim();
          if (HAS_CJK.test(c)) keys.add(c);
        }
      }
      walk(node.children, no, keys);
    } else if (node.type === NodeTypes.IF) {
      for (const branch of node.branches) walk(branch.children, skip, keys);
    } else if (node.type === NodeTypes.FOR) {
      walk(node.children, skip, keys);
    } else if (node.type === NodeTypes.TEXT && !skip) {
      const c = node.content.trim();
      if (HAS_CJK.test(c)) keys.add(c);
    }
  }
}

function extractVue(file, keys) {
  const code = readFileSync(file, 'utf8');
  const { descriptor } = parseSfc(code, { filename: file, pad: false });
  if (!descriptor.template?.content) return;
  try {
    const ast = baseParse(descriptor.template.content, { comments: true });
    walk(ast.children, false, keys);
  } catch {
    /* ignore */
  }
}

function readJson(path) {
  if (!existsSync(path)) return {};
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    return {};
  }
}

/** 稳定排序后写回；已有 value 保留 */
function mergeLocale(path, keys, fillNew) {
  const existing = readJson(path);
  let added = 0;
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(existing, key)) continue;
    existing[key] = fillNew(key);
    added++;
  }
  const sorted = Object.fromEntries(
    Object.keys(existing)
      .sort((a, b) => a.localeCompare(b, 'zh-CN'))
      .map((k) => [k, existing[k]]),
  );
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(sorted, null, 2)}\n`, 'utf8');
  return added;
}

const keys = new Set();
for (const file of walkDir(SRC)) extractVue(file, keys);

const zhPath = join(LOCALES, 'zh-CN.json');
const enPath = join(LOCALES, 'en-US.json');
const zhAdded = mergeLocale(zhPath, keys, (k) => k);
const enAdded = mergeLocale(enPath, keys, (k) => k);

console.log(
  `i18n:extract keys=${keys.size} zh+${zhAdded} en+${enAdded} → ${LOCALES}`,
);
