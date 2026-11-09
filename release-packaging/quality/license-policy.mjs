import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const require = createRequire(resolve('gui-web/node_modules/license-checker-rseidelsohn/package.json'));
const parse = require('spdx-expression-parse');
export function allowedLicense(expr, allow) {
  if (typeof expr !== 'string') return false;
  try {
    function evaluate(n) {
      if (n.conjunction === 'and') return evaluate(n.left) && evaluate(n.right);
      if (n.conjunction === 'or') return evaluate(n.left) || evaluate(n.right);
      if (n.plus) return false;
      return allow.has(n.license + (n.exception ? ` WITH ${n.exception}` : ''));
    }
    return evaluate(parse(expr));
  } catch { return false; }
}
