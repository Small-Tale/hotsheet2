export function routeMethods(source: string): Map<string, Record<string, string>>;
export function contractPathCount(contractText: string): number;
export function readmeRouteCount(readmeText: string): number | null;
export function syncContract(source: string, contractText: string, prettierOptions: object): Promise<string>;
export function contractPrettierOptions(): Promise<object>;
export function contractSyncErrors(source?: string, contractText?: string, readmeText?: string): Promise<string[]>;
