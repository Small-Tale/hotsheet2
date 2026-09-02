export function devReviewRequested(url: string, development: boolean): boolean {
  return development && new URL(url).searchParams.get('dev-review') === '1';
}
