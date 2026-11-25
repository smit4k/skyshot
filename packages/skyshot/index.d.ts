export interface RenderOptions {
  width?: number;
  scale?: 1 | 2 | 3;
  theme?: 'light' | 'dark' | 'dim';
  ancestors?: 0 | 1 | 2 | 3;
  timeZone?: string;
  hour12?: boolean;
}

export type PostJson = string | object;

export interface ProfileRenderOptions {
  width?: number;
  scale?: 1 | 2 | 3;
  theme?: 'light' | 'dark' | 'dim';
  layout?: 'auto' | 'mobile' | 'desktop';
}

export type ModerationAction = 'show' | 'blur' | 'hide';

export interface ProfileContext {
  viewerDid?: string;
  snapshotAt?: string;
  canChat?: boolean;
  labelerSubscribed?: boolean;
  betaUser?: boolean;
  liveAllowed?: boolean;
  descriptionFacets?: readonly object[];
  moderation?: {
    displayName?: ModerationAction;
    profileView?: ModerationAction;
    avatar?: ModerationAction;
    banner?: ModerationAction;
    status?: ModerationAction;
    links?: Record<string, ModerationAction>;
    knownFollowers?: Record<string, ModerationAction>;
    alerts?: readonly string[];
  };
}

export interface ProfileEnvelope {
  profile: object;
  context?: ProfileContext;
  labeler?: object;
  assets?: Map<string, Uint8Array>;
}

export interface ResolvedProfile extends ProfileEnvelope {
  context: ProfileContext;
  assets: Map<string, Uint8Array>;
}

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
  /** Load required fallback fonts in lazy mode; returns the supplied input. */
  prepare<T extends PostJson | ResolvedPost>(input: T): Promise<T>;
  resolve(input: string, options?: RenderOptions): Promise<ResolvedPost>;
  /** Resolve public profile data and the images visible at the requested width. */
  resolveProfile(input: string, options?: ProfileRenderOptions): Promise<ResolvedProfile>;
  /** Accept ProfileViewDetailed JSON, a profile envelope, or a resolved profile. */
  renderProfile(input: string | object | ProfileEnvelope, options?: ProfileRenderOptions): RenderResult;
  render(input: PostJson | ResolvedPost, options?: RenderOptions): RenderResult;
  /** Render up to eight layouts. Each renderMs includes PNG encoding and excludes batch setup. */
  renderMany(input: PostJson | ResolvedPost, options: readonly RenderOptions[]): (RenderResult & { renderMs: number })[];
}

export function loadSkyshot(options?: {
  /** Defer fallback fonts until resolve() or prepare(). Defaults to false. */
  lazy?: boolean;
  fonts?: Uint8Array[];
  fetch?: typeof globalThis.fetch;
}): Promise<Skyshot>;
