/** Browser-native exports for read-only call records. Keeping this local to
 *  the UI means exports never need an extra Tauri filesystem permission. */
function saveBlob(filename: string, blob: Blob): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

export function downloadText(filename: string, content: string): void {
  saveBlob(filename, new Blob([content], { type: "text/plain;charset=utf-8" }));
}

function escapePdf(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/\(/g, "\\(").replace(/\)/g, "\\)");
}

function wrap(value: string, width = 88): string[] {
  const words = value.replace(/\s+/g, " ").trim().split(" ");
  const lines: string[] = [];
  let line = "";
  for (const word of words) {
    if (word.length > width) {
      if (line) lines.push(line);
      lines.push(word.slice(0, width));
      line = word.slice(width);
    } else if (!line || `${line} ${word}`.length <= width) {
      line = line ? `${line} ${word}` : word;
    } else {
      lines.push(line);
      line = word;
    }
  }
  if (line) lines.push(line);
  return lines.length ? lines : [""];
}

/** Creates a compact, standards-compliant PDF using the built-in Helvetica
 *  font. Text is deliberately ASCII-normalized so every reader can render it. */
export function downloadPdf(filename: string, title: string, lines: string[]): void {
  const safeLines = lines.flatMap((line) => wrap(line.replace(/[^\x20-\x7E]/g, "?")));
  const pageLines = 44;
  const pages = Array.from({ length: Math.max(1, Math.ceil(safeLines.length / pageLines)) }, (_, index) =>
    safeLines.slice(index * pageLines, (index + 1) * pageLines),
  );
  const objects: string[] = ["<< /Type /Catalog /Pages 2 0 R >>", ""];
  const firstPageId = 3;
  const fontId = firstPageId + pages.length * 2;
  objects[1] = `<< /Type /Pages /Kids [${pages.map((_, index) => `${firstPageId + index * 2} 0 R`).join(" ")}] /Count ${pages.length} >>`;
  pages.forEach((page, index) => {
    const pageId = firstPageId + index * 2;
    const contentId = pageId + 1;
    const text = [
      "BT",
      "/F1 16 Tf",
      `72 760 Td (${escapePdf(title)}) Tj`,
      "/F1 10 Tf",
      "0 -26 Td",
      ...page.flatMap((line) => [`(${escapePdf(line)}) Tj`, "0 -15 Td"]),
      "ET",
    ].join("\n");
    objects[pageId - 1] = `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 ${fontId} 0 R >> >> /Contents ${contentId} 0 R >>`;
    objects[contentId - 1] = `<< /Length ${text.length} >>\nstream\n${text}\nendstream`;
  });
  objects[fontId - 1] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
  let pdf = "%PDF-1.4\n";
  const offsets: number[] = [0];
  objects.forEach((object, index) => {
    offsets.push(pdf.length);
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  offsets.slice(1).forEach((offset) => {
    pdf += `${offset.toString().padStart(10, "0")} 00000 n \n`;
  });
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF`;
  saveBlob(filename, new Blob([pdf], { type: "application/pdf" }));
}

export function recordFilename(title: string, suffix: string): string {
  const base = title.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "call";
  return `${base}-${suffix}`;
}
