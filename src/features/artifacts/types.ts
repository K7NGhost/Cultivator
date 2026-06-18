/**
 * High-level grouping used for filtering, navigation, and report sections.
 * A model's `category` should match its `kind`, for example `contact` uses
 * `contacts` and `browser_history` uses `browser`.
 */
export type ArtifactCategory =
  | "accounts"
  | "applications"
  | "browser"
  | "calls"
  | "contacts"
  | "credentials"
  | "files"
  | "locations"
  | "media"
  | "messages"
  | "notes"
  | "system"
  | "timeline"
  | "other";

export type ArtifactConfidence = "low" | "medium" | "high";

export type ArtifactSeverity = "info" | "low" | "medium" | "high" | "critical";

/**
 * Points back to the evidence that produced the artifact. `filePath` is the
 * primary source reference and optional line/column/offset values make the
 * artifact reviewable in file previews.
 */
export type ArtifactSourceReference = {
  datasourceId?: string;
  filePath: string;
  line?: number;
  column?: number;
  offset?: number;
  length?: number;
  parser?: string;
};

/**
 * Normalized timestamp attached to an artifact. Store `value` as an ISO-8601
 * string when possible; use `source` for parser-specific timestamp provenance.
 */
export type ArtifactTimestamp = {
  value: string;
  label?: string;
  timezone?: string;
  source?: string;
};

/**
 * Shared fields every artifact model can carry. Plugin payloads are persisted
 * as JSON, so these property names are intentionally stable and camelCase.
 */
export type BaseArtifact = {
  kind: string;
  category: ArtifactCategory;
  label: string;
  description?: string;
  source?: ArtifactSourceReference;
  timestamps?: ArtifactTimestamp[];
  tags?: string[];
  confidence?: ArtifactConfidence;
  severity?: ArtifactSeverity;
  raw?: unknown;
};

/** User account, profile, or service identity. */
export type AccountArtifact = BaseArtifact & {
  kind: "account";
  category: "accounts";
  username?: string;
  displayName?: string;
  email?: string;
  phone?: string;
  service?: string;
  identifier?: string;
};

/** Installed application or package metadata. */
export type ApplicationArtifact = BaseArtifact & {
  kind: "application";
  category: "applications";
  name: string;
  packageName?: string;
  version?: string;
  vendor?: string;
  installedAt?: string;
  lastUsedAt?: string;
};

/** Visited URL or browser navigation event. */
export type BrowserHistoryArtifact = BaseArtifact & {
  kind: "browser_history";
  category: "browser";
  url: string;
  title?: string;
  visitCount?: number;
  visitedAt?: string;
  browser?: string;
};

/** Call log entry. */
export type CallArtifact = BaseArtifact & {
  kind: "call";
  category: "calls";
  direction?: "incoming" | "outgoing" | "missed" | "unknown";
  phone?: string;
  contactName?: string;
  startedAt?: string;
  durationSeconds?: number;
};

/** Person, organization, or address book record. */
export type ContactArtifact = BaseArtifact & {
  kind: "contact";
  category: "contacts";
  name: string;
  phones?: string[];
  emails?: string[];
  organization?: string;
  notes?: string;
};

/** Credential indicator. Avoid storing raw secrets in artifact payloads. */
export type CredentialArtifact = BaseArtifact & {
  kind: "credential";
  category: "credentials";
  username?: string;
  service?: string;
  url?: string;
  secretType?: "password" | "token" | "key" | "cookie" | "unknown";
  secretPreview?: string;
};

/** File metadata, hash result, or discovered file reference. */
export type FileArtifact = BaseArtifact & {
  kind: "file";
  category: "files";
  path: string;
  name?: string;
  extension?: string;
  size?: number;
  sha256?: string;
  mimeType?: string;
};

/** Geographic coordinate with optional accuracy and source metadata. */
export type LocationArtifact = BaseArtifact & {
  kind: "location";
  category: "locations";
  latitude: number;
  longitude: number;
  altitude?: number;
  accuracyMeters?: number;
  recordedAt?: string;
  sourceApp?: string;
};

/** Image, video, audio, or other media file reference. */
export type MediaArtifact = BaseArtifact & {
  kind: "media";
  category: "media";
  path: string;
  mediaType?: "image" | "video" | "audio" | "other";
  createdAt?: string;
  width?: number;
  height?: number;
  durationSeconds?: number;
};

/** SMS, chat, email, or application message. */
export type MessageArtifact = BaseArtifact & {
  kind: "message";
  category: "messages";
  conversationId?: string;
  sender?: string;
  recipients?: string[];
  body?: string;
  sentAt?: string;
  receivedAt?: string;
  service?: string;
};

/** Plain text note, memo, or extracted note-like content. */
export type NoteArtifact = BaseArtifact & {
  kind: "note";
  category: "notes";
  title?: string;
  body: string;
  createdAt?: string;
  modifiedAt?: string;
};

/** System setting, property, or configuration value. */
export type SystemArtifact = BaseArtifact & {
  kind: "system";
  category: "system";
  key: string;
  value: string;
  namespace?: string;
};

/** Timestamped event for timeline correlation. */
export type TimelineArtifact = BaseArtifact & {
  kind: "timeline_event";
  category: "timeline";
  occurredAt: string;
  eventType: string;
  actor?: string;
  target?: string;
};

/** Fallback model for plugin-specific records that do not fit a known model. */
export type GenericArtifact = BaseArtifact & {
  kind: "record";
  category: "other";
  fields: Record<string, unknown>;
};

/** Discriminated union of every supported artifact payload model. */
export type Artifact =
  | AccountArtifact
  | ApplicationArtifact
  | BrowserHistoryArtifact
  | CallArtifact
  | ContactArtifact
  | CredentialArtifact
  | FileArtifact
  | LocationArtifact
  | MediaArtifact
  | MessageArtifact
  | NoteArtifact
  | SystemArtifact
  | TimelineArtifact
  | GenericArtifact;

/**
 * Field descriptor used by UI builders, validators, and documentation tables.
 * `type` describes the JSON value expected in plugin payloads.
 */
export type ArtifactModelField = {
  key: string;
  label: string;
  type: "string" | "number" | "boolean" | "datetime" | "string[]" | "json";
  required?: boolean;
};

/**
 * Runtime description of one artifact model. These definitions are the source
 * for artifact creation forms, validation hints, and API documentation.
 */
export type ArtifactModelDefinition = {
  kind: Artifact["kind"];
  category: ArtifactCategory;
  label: string;
  description: string;
  fields: ArtifactModelField[];
};

export type StoredArtifactRecord = {
  id: string;
  jobId: string;
  pluginId: string;
  datasourceId: string;
  filePath: string;
  resultKind: string;
  label: string;
  payload: Artifact | Record<string, unknown> | string | null;
  createdAt: string;
};
