import type { MediaAnnotation } from './api';

/** Intent implied by geometry when the reviewer has not added an explicit intent. */
export function annotationDefaultIntent(annotation: MediaAnnotation): string {
  switch (annotation.shape?.type) {
    case 'strike':
      return 'remove';
    case 'insertion':
      return 'insert';
    case 'arrow':
      return 'move';
    case 'rect':
    case 'freehand':
    case undefined:
      return 'comment';
  }
}

export function annotationPrimaryIntent(annotation: MediaAnnotation): string {
  const fallback = annotationDefaultIntent(annotation);
  return annotation.intents?.find((intent) => intent !== fallback) ?? fallback;
}

/** Named color family; unknown future intents retain their value and receive no assigned color. */
export function annotationIntentColor(annotation: MediaAnnotation): string | undefined {
  switch (annotationPrimaryIntent(annotation)) {
    case 'comment':
      return 'blue';
    case 'bug':
      return 'red';
    case 'change':
      return 'orange';
    case 'insert':
      return 'green';
    case 'remove':
      return 'purple';
    case 'move':
      return 'teal';
    case 'question':
      return 'yellow';
    default:
      return undefined;
  }
}
