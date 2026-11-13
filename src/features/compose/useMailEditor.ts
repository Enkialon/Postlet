import { onBeforeUnmount, shallowRef } from 'vue';
import { Editor } from '@tiptap/vue-3';
import StarterKit from '@tiptap/starter-kit';
import Placeholder from '@tiptap/extension-placeholder';
import Underline from '@tiptap/extension-underline';
import Link from '@tiptap/extension-link';

/** 会话级唯一 Tiptap 实例；模式切换不销毁 */
export function useMailEditor(options?: { placeholder?: string }) {
  const editor = shallowRef<Editor | null>(null);

  function ensureEditor(): Editor {
    if (editor.value) return editor.value;
    const instance = new Editor({
      extensions: [
        StarterKit.configure({
          heading: { levels: [1, 2, 3] },
          codeBlock: false,
          code: false,
        }),
        Underline,
        Link.configure({
          openOnClick: false,
          HTMLAttributes: { rel: 'noopener noreferrer', target: '_blank' },
        }),
        Placeholder.configure({
          placeholder: options?.placeholder ?? '写邮件…',
        }),
      ],
      content: '',
      editorProps: {
        attributes: {
          class:
            'mail-editor outline-none min-h-[2.5rem] text-[14px] leading-relaxed text-text-primary',
        },
      },
    });
    editor.value = instance;
    return instance;
  }

  function setHtml(html: string): void {
    const ed = ensureEditor();
    ed.commands.setContent(html || '', { emitUpdate: false });
  }

  function getHtml(): string {
    return editor.value?.getHTML() ?? '';
  }

  function isEmpty(): boolean {
    const ed = editor.value;
    if (!ed) return true;
    return ed.isEmpty;
  }

  function focus(): void {
    ensureEditor().commands.focus('end');
  }

  function clear(): void {
    ensureEditor().commands.clearContent(true);
  }

  function destroy(): void {
    editor.value?.destroy();
    editor.value = null;
  }

  onBeforeUnmount(destroy);

  return {
    editor,
    ensureEditor,
    setHtml,
    getHtml,
    isEmpty,
    focus,
    clear,
    destroy,
  };
}
