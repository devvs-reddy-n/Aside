/** Small text helpers shared by previews and editors. Pure, so they are tested on their own. */

export function wordCount(text: string): number {
  const words = text.trim().match(/\S+/g);
  return words ? words.length : 0;
}

export type Block = { kind: "para"; text: string } | { kind: "list"; items: string[] };

const BULLET = /^\s*(?:[-•*·▪◦]|\d+[.)])\s+/;

/**
 * Splits plain text into paragraphs and bullet lists, so previews can show
 * real bullets. A line starting with "-", "•", "*" (or "1.") is a list item;
 * consecutive items form one list. Blank lines separate paragraphs.
 */
export function toBlocks(text: string): Block[] {
  const blocks: Block[] = [];
  let para: string[] = [];
  const flushPara = () => {
    if (para.length) blocks.push({ kind: "para", text: para.join("\n") });
    para = [];
  };
  for (const raw of text.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      flushPara();
      continue;
    }
    if (BULLET.test(line)) {
      flushPara();
      const item = line.replace(BULLET, "").trim();
      const last = blocks[blocks.length - 1];
      if (last && last.kind === "list") last.items.push(item);
      else blocks.push({ kind: "list", items: [item] });
      continue;
    }
    const last = blocks[blocks.length - 1];
    if (para.length === 0 && last && last.kind === "list" && /^\s{2,}/.test(raw)) {
      last.items[last.items.length - 1] += ` ${line.trim()}`;
      continue;
    }
    para.push(line.trim());
  }
  flushPara();
  return blocks;
}

/** "devendra_Resume_Senior_Call-Analyst.pdf" → "devendra Resume Senior Call-Analyst". */
export function displayFileName(fileName: string): string {
  const base = fileName.replace(/\.[a-z0-9]{1,5}$/i, "");
  const cleaned = base.replace(/[_]+/g, " ").replace(/\s{2,}/g, " ").trim();
  return cleaned || fileName;
}

/** "PDF", "Word", "Text" or the extension in capitals. */
export function fileKind(fileName: string, mime?: string | null): string {
  const ext = fileName.match(/\.([a-z0-9]{1,5})$/i)?.[1]?.toLowerCase() ?? "";
  if (ext === "pdf" || mime === "application/pdf") return "PDF";
  if (ext === "docx" || ext === "doc") return "Word";
  if (ext === "txt" || ext === "md" || (!ext && mime?.startsWith("text/"))) return "Text";
  return ext ? ext.toUpperCase() : "Text";
}

/** A web link worth trying to import. */
export function isWebUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname.includes(".");
  } catch {
    return false;
  }
}
