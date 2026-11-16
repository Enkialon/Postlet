/**
 * 编译期：Vue 模板静态中文 → $t('原文')。
 * 跳过 v-no-i18n；不扫描运行时 DOM；不在 build 时覆盖翻译文件。
 * 抽词条请用：npm run i18n:extract
 */
import type { Plugin } from 'vite';
import { parse as parseSfc } from '@vue/compiler-sfc';
import {
  NodeTypes,
  baseParse,
  type AttributeNode,
  type ElementNode,
  type RootNode,
  type TemplateChildNode,
} from '@vue/compiler-dom';
import MagicString from 'magic-string';

const HAS_CJK = /[\u4e00-\u9fff]/
const I18N_ATTRS = new Set([
  'title',
  'placeholder',
  'aria-label',
  'alt',
  'aria-placeholder',
  'aria-description',
]);

export function escapeMsgKey(text: string): string {
  return text.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}

function hasNoI18n(node: ElementNode): boolean {
  return node.props.some((p) => {
    if (p.type === NodeTypes.DIRECTIVE && p.name === 'no-i18n') return true;
    if (p.type === NodeTypes.ATTRIBUTE && p.name === 'v-no-i18n') return true;
    return false;
  });
}

/** 遍历模板 AST，收集/回调需国际化的静态文案 */
export function walkTemplate(
  nodes: TemplateChildNode[],
  inNoI18n: boolean,
  onText: (start: number, end: number, raw: string) => void,
  onAttr: (prop: AttributeNode, name: string, value: string) => void,
): void {
  for (const node of nodes) {
    if (node.type === NodeTypes.ELEMENT) {
      const skip = inNoI18n || hasNoI18n(node);
      if (!skip) {
        for (const prop of node.props) {
          if (prop.type !== NodeTypes.ATTRIBUTE) continue;
          if (!I18N_ATTRS.has(prop.name) || !prop.value) continue;
          const content = prop.value.content;
          if (HAS_CJK.test(content)) onAttr(prop, prop.name, content);
        }
      }
      if (node.children.length) {
        walkTemplate(node.children, skip, onText, onAttr);
      }
      continue;
    }
    if (node.type === NodeTypes.IF) {
      for (const branch of node.branches) {
        walkTemplate(branch.children, inNoI18n, onText, onAttr);
      }
      continue;
    }
    if (node.type === NodeTypes.FOR) {
      walkTemplate(node.children, inNoI18n, onText, onAttr);
      continue;
    }
    if (
      node.type === NodeTypes.TEXT &&
      !inNoI18n &&
      HAS_CJK.test(node.content)
    ) {
      onText(node.loc.start.offset, node.loc.end.offset, node.content);
    }
  }
}

/** 从单个 Vue SFC 抽出模板中文 Key（去空） */
export function extractKeysFromVueSfc(code: string): string[] {
  const { descriptor } = parseSfc(code, { pad: false });
  const template = descriptor.template;
  if (!template?.content) return [];

  let ast: RootNode;
  try {
    ast = baseParse(template.content, { comments: true });
  } catch {
    return [];
  }

  const keys = new Set<string>();
  walkTemplate(
    ast.children,
    false,
    (_s, _e, raw) => {
      const t = raw.trim();
      if (t) keys.add(t);
    },
    (_prop, _name, value) => {
      const t = value.trim();
      if (t) keys.add(t);
    },
  );
  return [...keys];
}

function transformTemplate(
  code: string,
  templateOffset: number,
  ast: RootNode,
): MagicString | null {
  const s = new MagicString(code);
  let changed = false;

  walkTemplate(
    ast.children,
    false,
    (start, end, raw) => {
      const trimmed = raw.trim();
      if (!trimmed) return;
      const leading = raw.match(/^\s*/)?.[0] ?? '';
      const trailing = raw.match(/\s*$/)?.[0] ?? '';
      s.overwrite(
        templateOffset + start,
        templateOffset + end,
        `${leading}{{ $t('${escapeMsgKey(trimmed)}') }}${trailing}`,
      );
      changed = true;
    },
    (prop, name, value) => {
      s.overwrite(
        templateOffset + prop.loc.start.offset,
        templateOffset + prop.loc.end.offset,
        `:${name}="$t('${escapeMsgKey(value)}')"`,
      );
      changed = true;
    },
  );

  return changed ? s : null;
}

export function autoI18nPlugin(): Plugin {
  return {
    name: 'vite-plugin-auto-i18n',
    enforce: 'pre',
    transform(code, id) {
      const file = id.split('?')[0] ?? id;
      if (!file.endsWith('.vue')) return null;

      const { descriptor } = parseSfc(code, {
        filename: file,
        pad: false,
      });
      const template = descriptor.template;
      if (!template?.content) return null;

      let ast: RootNode;
      try {
        ast = baseParse(template.content, { comments: true });
      } catch {
        return null;
      }

      const edited = transformTemplate(code, template.loc.start.offset, ast);
      if (!edited) return null;
      return {
        code: edited.toString(),
        map: edited.generateMap({ hires: true, source: file }),
      };
    },
  };
}
