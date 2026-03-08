function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function wrapSpan(cls: string, text: string): string {
  return `<span class="${cls}">${escapeHtml(text)}</span>`;
}

export function highlightToml(toml: string): string {
  return toml
    .split("\n")
    .map((line) => {
      // Comments
      if (/^\s*#/.test(line)) {
        return wrapSpan("tok-comment", line);
      }

      // Section headers [[...]] or [...]
      if (/^\s*\[{1,2}[^\]]+\]{1,2}\s*$/.test(line)) {
        return wrapSpan("tok-section", line);
      }

      // Key = value lines
      const kvMatch = line.match(/^(\s*)([\w.]+)(\s*=\s*)(.*)/);
      if (kvMatch) {
        const [, indent, key, op, value] = kvMatch;
        const highlightedValue = highlightValue(value);
        return `${escapeHtml(indent)}${wrapSpan("tok-key", key)}${wrapSpan("tok-op", op)}${highlightedValue}`;
      }

      return escapeHtml(line);
    })
    .join("\n");
}

function highlightValue(value: string): string {
  const trimmed = value.trim();

  // String
  if (/^".*"$/.test(trimmed)) {
    return wrapSpan("tok-string", value);
  }

  // Boolean
  if (/^(true|false)$/.test(trimmed)) {
    return wrapSpan("tok-boolean", value);
  }

  // Number
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return wrapSpan("tok-number", value);
  }

  // Array — highlight inline
  if (trimmed.startsWith("[")) {
    return highlightArray(value);
  }

  return escapeHtml(value);
}

function highlightArray(value: string): string {
  // Tokenize preserving structure, highlighting strings and numbers within
  return value.replace(
    /("(?:[^"\\]|\\.)*")|(\b\d+\b)|(true|false)|(\{)|(\})|([[\],=])|(\w+)(?=\s*=)/g,
    (match, str, num, bool, _lb, _rb, _punct, key) => {
      if (str) return wrapSpan("tok-string", str);
      if (num) return wrapSpan("tok-number", num);
      if (bool) return wrapSpan("tok-boolean", bool);
      if (key) return wrapSpan("tok-key", key);
      return escapeHtml(match);
    }
  );
}
