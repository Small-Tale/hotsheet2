const REMIFY_START = 'remify(';
const PIXEL_LITERAL = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+))px$/i;

function greatestCommonDivisor(left, right) {
  while (right !== 0n) [left, right] = [right, left % right];
  return left;
}

function dividePixelLiteralBySixteen(raw) {
  const negative = raw.startsWith('-');
  const unsigned = raw.replace(/^[+-]/, '');
  const [integer = '0', fraction = ''] = unsigned.split('.');
  let numerator = BigInt(`${integer || '0'}${fraction}`);
  let denominator = 16n * 10n ** BigInt(fraction.length);

  if (numerator === 0n) return '0rem';
  const divisor = greatestCommonDivisor(numerator, denominator);
  numerator /= divisor;
  denominator /= divisor;

  const whole = numerator / denominator;
  let remainder = numerator % denominator;
  let decimal = '';
  while (remainder !== 0n) {
    remainder *= 10n;
    decimal += String(remainder / denominator);
    remainder %= denominator;
  }

  return `${negative ? '-' : ''}${whole}${decimal ? `.${decimal}` : ''}rem`;
}

function remifyError(argument, context) {
  return new Error(`remify() accepts one numeric px literal; received "${argument}" in ${context}`);
}

export function transformRemifyValue(value, context = 'CSS value') {
  let output = '';
  let index = 0;

  while (index < value.length) {
    const character = value[index];

    if (character === '"' || character === "'") {
      const quote = character;
      const start = index++;
      while (index < value.length) {
        if (value[index] === '\\') index += 2;
        else if (value[index++] === quote) break;
      }
      output += value.slice(start, index);
      continue;
    }

    if (value.startsWith('/*', index)) {
      const end = value.indexOf('*/', index + 2);
      const next = end === -1 ? value.length : end + 2;
      output += value.slice(index, next);
      index = next;
      continue;
    }

    const previous = index === 0 ? '' : value[index - 1];
    if (value.startsWith(REMIFY_START, index) && !/[\w-]/.test(previous)) {
      const end = value.indexOf(')', index + REMIFY_START.length);
      if (end === -1) throw remifyError(value.slice(index + REMIFY_START.length), context);
      const argument = value.slice(index + REMIFY_START.length, end).trim();
      const match = PIXEL_LITERAL.exec(argument);
      if (!match) throw remifyError(argument, context);
      output += dividePixelLiteralBySixteen(match[1]);
      index = end + 1;
      continue;
    }

    output += character;
    index += 1;
  }

  return output;
}

export default function remifyCss() {
  const transform = (node, field) => {
    try {
      node[field] = transformRemifyValue(node[field], `${node.type} ${node.toString()}`);
    } catch (error) {
      throw node.error(error instanceof Error ? error.message : String(error));
    }
  };

  return {
    postcssPlugin: 'kerf-remify',
    Declaration: (declaration) => transform(declaration, 'value'),
    AtRule: (atRule) => transform(atRule, 'params'),
  };
}
