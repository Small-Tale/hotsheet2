const modernColorFunction = /\b(?:color-mix|oklab|oklch|lab|lch|color)\(/gi;

const renderedColorProperties = [
  'background-color',
  'background-image',
  'border-bottom-color',
  'border-left-color',
  'border-right-color',
  'border-top-color',
  'box-shadow',
  'caret-color',
  'color',
  'column-rule-color',
  'fill',
  'flood-color',
  'outline-color',
  'stop-color',
  'stroke',
  'text-decoration-color',
  'text-emphasis-color',
  'text-shadow',
  '-webkit-text-fill-color',
  '-webkit-text-stroke-color',
] as const;

/** Replace modern CSS color functions embedded in simple colors, shadows, or gradients. */
export function replaceModernColorFunctions(value: string, convert: (color: string) => string | undefined): string {
  let cursor = 0;
  let result = '';
  modernColorFunction.lastIndex = 0;
  for (let match = modernColorFunction.exec(value); match; match = modernColorFunction.exec(value)) {
    const start = match.index;
    const openingParenthesis = start + match[0].length - 1;
    let depth = 0;
    let end = openingParenthesis;
    for (; end < value.length; end += 1) {
      if (value[end] === '(') depth += 1;
      else if (value[end] === ')' && --depth === 0) break;
    }
    if (depth !== 0) break;
    const source = value.slice(start, end + 1);
    const replacement = convert(source);
    if (!replacement) continue;
    result += value.slice(cursor, start) + replacement;
    cursor = end + 1;
    modernColorFunction.lastIndex = cursor;
  }
  return cursor === 0 ? value : result + value.slice(cursor);
}

/** Make cloned computed styles consumable by html2canvas 1.4's legacy color parser. */
export function normalizeCaptureColors(document: Document): void {
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext('2d', { willReadFrequently: true });
  const view = document.defaultView;
  if (!context || !view) return;

  const convert = (color: string): string => {
    context.clearRect(0, 0, 1, 1);
    context.fillStyle = '#010203';
    context.fillStyle = color;
    context.fillRect(0, 0, 1, 1);
    const [red, green, blue, alpha] = context.getImageData(0, 0, 1, 1).data;
    return `rgba(${red}, ${green}, ${blue}, ${alpha / 255})`;
  };

  for (const element of document.querySelectorAll<HTMLElement>('*')) {
    const computed = view.getComputedStyle(element);
    for (const property of renderedColorProperties) {
      const value = computed.getPropertyValue(property);
      modernColorFunction.lastIndex = 0;
      if (!modernColorFunction.test(value)) continue;
      modernColorFunction.lastIndex = 0;
      const normalized = replaceModernColorFunctions(value, convert);
      if (normalized !== value) element.style.setProperty(property, normalized, 'important');
    }
  }
}
