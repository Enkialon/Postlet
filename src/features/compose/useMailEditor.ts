import { onBeforeUnmount, shallowRef } from 'vue';
import { Editor } from '@tiptap/vue-3';
import StarterKit from '@tiptap/starter-kit';
import Placeholder from '@tiptap/extension-placeholder';
import Underline from '@tiptap/extension-underline';
import Link from '@tiptap/extension-link';
import Image from '@tiptap/extension-image';

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ''));
    reader.onerror = () => reject(reader.error ?? new Error('读取图片失败'));
    reader.readAsDataURL(file);
  });
}

function collectImageFiles(
  data: DataTransfer | null | undefined,
): File[] {
  if (!data) return [];
  const fromItems: File[] = [];
  if (data.items?.length) {
    for (const item of Array.from(data.items)) {
      if (item.kind === 'file' && item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) fromItems.push(file);
      }
    }
  }
  if (fromItems.length) return fromItems;
  return Array.from(data.files ?? []).filter((f) =>
    f.type.startsWith('image/'),
  );
}

async function insertImages(editor: Editor, files: File[]): Promise<void> {
  for (const file of files) {
    const src = await readFileAsDataUrl(file);
    if (!src) continue;
    editor
      .chain()
      .focus()
      .setImage({ src, alt: file.name || '粘贴图片' })
      .run();
  }
}

/** 会话级唯一 Tiptap 实例；模式切换不销毁 */
export function useMailEditor(options?: { placeholder?: string }) {
  const editor = shallowRef<Editor | null>(null);

  function ensureEditor(): Editor {
    if (editor.value) return editor.value;

    let instance!: Editor;
    instance = new Editor({
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
        Image.configure({
          inline: false,
          allowBase64: true,
          HTMLAttributes: {
            class: 'mail-editor-image',
          },
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
        handlePaste(_view, event) {
          const files = collectImageFiles(event.clipboardData);
          if (!files.length) return false;
          event.preventDefault();
          void insertImages(instance, files);
          return true;
        },
        handleDrop(_view, event) {
          const files = collectImageFiles(event.dataTransfer);
          if (!files.length) return false;
          event.preventDefault();
          void insertImages(instance, files);
          return true;
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
