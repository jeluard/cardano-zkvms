/**
 * Lightweight Aiken syntax highlighter for the overlay editor.
 *
 * Produces an HTML string with <span class="hl-*"> wrappers suitable for
 * insertion into a <pre><code> element layered behind a transparent textarea.
 */

const KEYWORDS = new Set([
  'fn', 'if', 'else', 'let', 'when', 'is', 'test', 'use', 'as',
  'pub', 'type', 'opaque', 'const', 'validator', 'expect', 'check',
  'fail', 'trace', 'and', 'or', 'not', 'todo', 'match',
]);

const BUILTINS = new Set([
  'True', 'False', 'None', 'Some', 'Ok', 'Err',
]);

const TYPES = new Set([
  'Int', 'Bool', 'String', 'ByteArray', 'List', 'Option', 'Result',
  'Void', 'Data', 'Pairs', 'PRNG',
]);

// Order matters: earlier rules are tried first.
const TOKEN_RE = new RegExp([
  '(\\/\\/[^\\n]*)',           // 1: line comment
  '("(?:[^"\\\\]|\\\\.)*")',   // 2: string
  '(#"[0-9a-fA-F]*")',        // 3: byte literal
  '(\\b\\d[\\d_]*\\b)',       // 4: number
  '(\\b[A-Z][A-Za-z0-9_]*)', // 5: upper-case identifier (type / constructor)
  '(\\b[a-z_][A-Za-z0-9_]*)', // 6: lower-case identifier (keyword / name)
  '(->|>=|<=|==|!=|&&|\\|\\||\\|>|\\.\\.)', // 7: operators
].join('|'), 'g');

/**
 * Highlight Aiken source code, returning an HTML string.
 * All text is HTML-escaped; recognised tokens are wrapped in spans.
 */
export function highlightAiken(src) {
  const parts = [];
  let last = 0;

  TOKEN_RE.lastIndex = 0;
  let m;
  while ((m = TOKEN_RE.exec(src)) !== null) {
    // Emit any plain text between the previous match and this one
    if (m.index > last) {
      parts.push(esc(src.slice(last, m.index)));
    }

    if (m[1]) {                          // comment
      parts.push(`<span class="hl-comment">${esc(m[0])}</span>`);
    } else if (m[2]) {                   // string
      parts.push(`<span class="hl-string">${esc(m[0])}</span>`);
    } else if (m[3]) {                   // byte literal
      parts.push(`<span class="hl-string">${esc(m[0])}</span>`);
    } else if (m[4]) {                   // number
      parts.push(`<span class="hl-number">${esc(m[0])}</span>`);
    } else if (m[5]) {                   // upper-case identifier
      const cls = BUILTINS.has(m[0]) ? 'hl-builtin'
                : TYPES.has(m[0])    ? 'hl-type'
                :                      'hl-type'; // treat unknown Uppercase as type
      parts.push(`<span class="${cls}">${esc(m[0])}</span>`);
    } else if (m[6]) {                   // lower-case identifier
      if (KEYWORDS.has(m[0])) {
        parts.push(`<span class="hl-keyword">${esc(m[0])}</span>`);
      } else {
        parts.push(esc(m[0]));
      }
    } else if (m[7]) {                   // operator
      parts.push(`<span class="hl-operator">${esc(m[0])}</span>`);
    } else {
      parts.push(esc(m[0]));
    }

    last = m.index + m[0].length;
  }

  // Remaining text after the last match
  if (last < src.length) {
    parts.push(esc(src.slice(last)));
  }

  return parts.join('');
}

function esc(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
