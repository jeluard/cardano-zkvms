/**
 * Lightweight UPLC syntax highlighter.
 *
 * Produces HTML strings with <span class="hl-*"> wrappers for UPLC
 * debug output (Program, Constant, Variable, Apply, etc.).
 */

const UPLC_KEYWORDS = new Set([
  'Program', 'Constant', 'Variable', 'LamAbs', 'Apply',
  'Force', 'Delay', 'Case', 'Constr', 'version', 'program',
  'term', 'args', 'clauses', 'default', 'fields', 'index',
  'DeBruijn', 'Version',
]);

const UPLC_BUILTINS = new Set([
  'Integer', 'Bool', 'ByteString', 'String', 'Unit', 'List', 'Data',
  'true', 'false', 'None', 'Some', 'Void',
]);

// Order matters: earlier rules are tried first.
const TOKEN_RE = new RegExp([
  '(\\/\\/[^\\n]*)',           // 1: line comment
  ('(--[^\\n]*)'),             // 1b: UPLC line comment
  ('(\\{-[^-]*-\\})'),         // 2: UPLC block comment
  ('("(?:[^"\\\\]|\\\\.)*")'), // 3: string
  ('(0x[0-9a-fA-F]+)'),        // 4: hex number
  ('(\\b\\d[\\d_]*\\b)'),      // 5: decimal number
  ('(\\b[A-Z][A-Za-z0-9_]*)'), // 6: upper-case identifier (type / constructor)
  ('(\\b[a-z_][A-Za-z0-9_]*)'), // 7: lower-case identifier
  ('([()\\[\\]{}:=,;]|->|\\\\)'), // 8: punctuation and operators
].join('|'), 'g');

/**
 * Highlight UPLC source code, returning an HTML string.
 */
export function highlightUplc(src) {
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
    } else if (m[2]) {                   // block comment
      parts.push(`<span class="hl-comment">${esc(m[0])}</span>`);
    } else if (m[3]) {                   // string
      parts.push(`<span class="hl-string">${esc(m[0])}</span>`);
    } else if (m[4]) {                   // hex number
      parts.push(`<span class="hl-number">${esc(m[0])}</span>`);
    } else if (m[5]) {                   // decimal number
      parts.push(`<span class="hl-number">${esc(m[0])}</span>`);
    } else if (m[6]) {                   // upper-case identifier
      const cls = UPLC_KEYWORDS.has(m[0]) ? 'hl-keyword'
                : UPLC_BUILTINS.has(m[0]) ? 'hl-builtin'
                :                            'hl-type';
      parts.push(`<span class="${cls}">${esc(m[0])}</span>`);
    } else if (m[7]) {                   // lower-case identifier
      if (UPLC_KEYWORDS.has(m[0])) {
        parts.push(`<span class="hl-keyword">${esc(m[0])}</span>`);
      } else if (UPLC_BUILTINS.has(m[0])) {
        parts.push(`<span class="hl-builtin">${esc(m[0])}</span>`);
      } else {
        parts.push(esc(m[0]));
      }
    } else if (m[8]) {                   // punctuation
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
