export interface RenderOptions {
  width?: number;
  scale?: 1 | 2 | 3;
  theme?: 'light' | 'dark' | 'dim';
  ancestors?: 0 | 1 | 2 | 3;
  timeZone?: string;
  hour12?: boolean;
}

export type PostJson = string | object;

export interface ResolvedPost {
  post: PostJson;
  assets: Map<string, Uint8Array>;
}

export type Warning =
  | { code: 'invalidFacet' }
  | { code: 'missingAsset' | 'invalidAsset' | 'unsupportedEmbed'; detail: string };

export interface RenderResult {
  png: Uint8Array;
  width: number;
  height: number;
  warnings: Warning[];
}

export interface Skyshot {
  resolve(input: string, options?: RenderOptions): Promise<ResolvedPost>;
  render(input: PostJson | ResolvedPost, options?: RenderOptions): RenderResult;
}

export function loadSkyshot(options?: {
  fonts?: Uint8Array[];
  fetch?: typeof globalThis.fetch;
}): Promise<Skyshot>;
