import { useState, type ReactNode } from "react";
import type { ParsedCodeBlock } from "../../core/answerParse";
import { Icon } from "../../kit/Icon";

const KEYWORDS = new Set(
  ("const let var function return if else for while class new import from export default async await try catch throw switch case break continue " +
    "def in is not and or None True False lambda with as yield pass raise fn pub impl struct enum match mut use mod self trait type interface " +
    "public private static void int string bool null true false undefined this extends implements package final SELECT FROM WHERE JOIN GROUP BY ORDER LIMIT INSERT UPDATE DELETE")
    .split(" "),
);

const TOKEN = /(\/\/[^\n]*|#[^\n]*|--[^\n]*)|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\b\d[\d_.]*\b)|([A-Za-z_][A-Za-z0-9_]*)/g;

/** A deliberately small highlighter: enough colour to scan code, no per-language rules to maintain. */
function highlightLine(line: string): ReactNode[] {
  const parts: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const match of line.matchAll(TOKEN)) {
    const index = match.index ?? 0;
    if (index > last) parts.push(line.slice(last, index));
    const [text, comment, string, number, word] = match;
    if (comment) parts.push(<i key={key++} className="tok-com">{text}</i>);
    else if (string) parts.push(<i key={key++} className="tok-str">{text}</i>);
    else if (number) parts.push(<i key={key++} className="tok-num">{text}</i>);
    else if (word && KEYWORDS.has(word)) parts.push(<i key={key++} className="tok-key">{text}</i>);
    else parts.push(text);
    last = index + text.length;
  }
  if (last < line.length) parts.push(line.slice(last));
  return parts;
}

/** Code shown as code: its own scroll sideways, line numbers, one-tap copy. */
export function Code({ block, streaming }: { block: ParsedCodeBlock; streaming?: boolean }) {
  const [copied, setCopied] = useState(false);
  const lines = block.code.split("\n");

  function copy() {
    void navigator.clipboard
      .writeText(block.code)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1400);
      })
      .catch(() => undefined);
  }

  return (
    <figure className="lv-code selectable">
      <figcaption>
        <span><i className="lv-code__dots" aria-hidden="true"><b /><b /><b /></i>{block.language ?? "code"}</span>
        <button type="button" onClick={copy}><Icon name={copied ? "check" : "copy"} size={13} />{copied ? "Copied" : "Copy"}</button>
      </figcaption>
      <pre tabIndex={0}>
        {lines.map((line, index) => (
          <span className="lv-code__line" key={index}>{highlightLine(line)}{"\n"}</span>
        ))}
        {streaming ? <span className="lv-caret" aria-hidden="true" /> : null}
      </pre>
    </figure>
  );
}
