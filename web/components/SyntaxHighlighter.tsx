import type { ReactNode } from "react";

export type HighlightLanguage = "cobble" | "mcfunction" | "json" | "text";

const keywords = new Set([
  "and",
  "as",
  "break",
  "class",
  "continue",
  "def",
  "elif",
  "else",
  "false",
  "for",
  "from",
  "if",
  "import",
  "in",
  "none",
  "not",
  "or",
  "pass",
  "return",
  "true",
  "while"
]);

const helpers = new Set([
  "datapack",
  "event",
  "range",
  "stdlib",
  "text"
]);

const minecraftCommands = new Set([
  "advancement",
  "attribute",
  "bossbar",
  "clear",
  "clone",
  "damage",
  "data",
  "datapack",
  "dialog",
  "effect",
  "enchant",
  "execute",
  "experience",
  "fetchprofile",
  "fill",
  "function",
  "gamemode",
  "gamerule",
  "give",
  "item",
  "loot",
  "particle",
  "playsound",
  "recipe",
  "return",
  "say",
  "scoreboard",
  "setblock",
  "summon",
  "tag",
  "team",
  "tellraw",
  "title",
  "tp",
  "version",
  "waypoint"
]);

const tokenPattern =
  /("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|#[^\n]*|@[a-z](?:\[[^\]\n]*\])?|\/?[A-Za-z_][A-Za-z0-9_.:-]*|\b\d+(?:\.\d+)?|\.\.|[{}\[\]():,=+\-*<>/])/g;

type CodeBlockProps = {
  code: string;
  language: HighlightLanguage;
  className?: string;
};

export function CodeBlock({ code, language, className }: CodeBlockProps) {
  return (
    <pre className={["syntax-code", className].filter(Boolean).join(" ")}>
      <code>
        <HighlightedCode code={code} language={language} />
      </code>
    </pre>
  );
}

export function HighlightedCode({
  code,
  language
}: {
  code: string;
  language: HighlightLanguage;
}) {
  return <>{highlight(code, language)}</>;
}

function highlight(code: string, language: HighlightLanguage) {
  if (language === "text") {
    return [code];
  }

  const nodes: ReactNode[] = [];
  let cursor = 0;

  for (const match of code.matchAll(tokenPattern)) {
    const token = match[0];
    const index = match.index ?? 0;

    if (index > cursor) {
      nodes.push(code.slice(cursor, index));
    }

    const className = classifyToken(code, token, index, index + token.length, language);
    nodes.push(
      className ? (
        <span className={`tok ${className}`} key={`${index}-${token}`}>
          {token}
        </span>
      ) : (
        token
      )
    );
    cursor = index + token.length;
  }

  if (cursor < code.length) {
    nodes.push(code.slice(cursor));
  }

  return nodes;
}

function classifyToken(
  code: string,
  token: string,
  start: number,
  end: number,
  language: HighlightLanguage
) {
  const lower = token.toLowerCase();

  if (token.startsWith("#")) {
    return "tok-comment";
  }

  if (isQuoted(token)) {
    return language === "json" && isJsonKey(code, end) ? "tok-key" : "tok-string";
  }

  if (token.startsWith("@")) {
    return "tok-selector";
  }

  if (/^\d/.test(token)) {
    return "tok-number";
  }

  if (lower === "true" || lower === "false" || lower === "null" || lower === "none") {
    return "tok-constant";
  }

  if (token.startsWith("/") && minecraftCommands.has(lower.slice(1))) {
    return "tok-command";
  }

  if (language === "mcfunction" && isLineCommand(code, start) && minecraftCommands.has(lower)) {
    return "tok-command";
  }

  if (minecraftCommands.has(lower) && language !== "json") {
    return "tok-command";
  }

  if (isResourceId(token)) {
    return "tok-resource";
  }

  if (keywords.has(lower)) {
    return "tok-keyword";
  }

  if (helpers.has(token.split(".")[0])) {
    return "tok-helper";
  }

  if (code[end] === "(" && language !== "json") {
    return "tok-call";
  }

  if (/^[{}\[\]():,=+\-*<>/]|\.{2}$/.test(token)) {
    return "tok-punctuation";
  }

  return "";
}

function isQuoted(token: string) {
  return (
    (token.startsWith("\"") && token.endsWith("\"")) ||
    (token.startsWith("'") && token.endsWith("'"))
  );
}

function isJsonKey(code: string, end: number) {
  return /^\s*:/.test(code.slice(end));
}

function isLineCommand(code: string, start: number) {
  const lineStart = code.lastIndexOf("\n", start - 1) + 1;
  return code.slice(lineStart, start).trim().length === 0;
}

function isResourceId(token: string) {
  return /^[A-Za-z0-9_.-]+:[A-Za-z0-9_./-]+$/.test(token.replace(/^\//, ""));
}
