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
  | "calendar"
  | "communications"
  | "journeys"
  | "maps"
  | "networks"
  | "notes"
  | "search"
  | "system"
  | "timeline"
  | "other"
  | (string & {});

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

export type ArtifactGroup = {
  id: string;
  label: string;
};

export type ArtifactDeduplicationMode = "group" | "preserve";

/**
 * Controls how logically identical artifact entries are presented. Grouping is
 * non-destructive: every stored occurrence and source reference is retained.
 */
export type ArtifactDeduplicationPolicy = {
  mode?: ArtifactDeduplicationMode;
  identityFields?: string[];
};

/**
 * Shared fields every artifact model can carry. Plugin payloads are persisted
 * as JSON, so these property names are intentionally stable and camelCase.
 */
export type BaseArtifact = {
  kind: string;
  category: ArtifactCategory;
  label: string;
  deduplication?: ArtifactDeduplicationPolicy;
  icon?: string;
  group?: ArtifactGroup;
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

export type NodeArtifact<
  Kind extends string,
  Category extends ArtifactCategory,
  Fields extends Record<string, unknown> = Record<string, unknown>,
> = BaseArtifact &
  Fields & {
    kind: Kind;
    category: Category;
    nodeId?: string;
    relatedIds?: string[];
  };

export type SmsArtifact = NodeArtifact<
  "sms",
  "messages",
  {
    conversationId?: string;
    sender?: string;
    recipients?: string[];
    body?: string;
    sentAt?: string;
    receivedAt?: string;
    service?: string;
  }
>;

export type EmailArtifact = NodeArtifact<
  "email",
  "communications",
  {
    messageId?: string;
    from?: string;
    to?: string[];
    cc?: string[];
    bcc?: string[];
    subject?: string;
    body?: string;
    sentAt?: string;
    receivedAt?: string;
  }
>;

export type MmsArtifact = NodeArtifact<
  "mms",
  "messages",
  {
    conversationId?: string;
    sender?: string;
    recipients?: string[];
    body?: string;
    attachments?: string[];
    sentAt?: string;
    receivedAt?: string;
  }
>;

export type ChatArtifact = NodeArtifact<
  "chat",
  "communications",
  {
    conversationId?: string;
    participants?: string[];
    service?: string;
    title?: string;
    startedAt?: string;
    lastMessageAt?: string;
  }
>;

export type InstantMessageArtifact = NodeArtifact<
  "instant_message",
  "messages",
  {
    conversationId?: string;
    sender?: string;
    recipients?: string[];
    body?: string;
    sentAt?: string;
    receivedAt?: string;
    service?: string;
  }
>;

export type UserAccountArtifact = NodeArtifact<
  "user_account",
  "accounts",
  {
    username?: string;
    displayName?: string;
    email?: string;
    phone?: string;
    service?: string;
    identifier?: string;
  }
>;

export type VoiceMailArtifact = NodeArtifact<
  "voice_mail",
  "communications",
  {
    phone?: string;
    contactName?: string;
    transcript?: string;
    receivedAt?: string;
    durationSeconds?: number;
    audioPath?: string;
  }
>;

export type CalendarEntryArtifact = NodeArtifact<
  "calendar_entry",
  "calendar",
  {
    title?: string;
    startsAt?: string;
    endsAt?: string;
    location?: string;
    attendees?: string[];
    organizer?: string;
  }
>;

export type PasswordArtifact = NodeArtifact<
  "password",
  "credentials",
  {
    username?: string;
    service?: string;
    url?: string;
    secretPreview?: string;
    storedAt?: string;
  }
>;

export type JourneyArtifact = NodeArtifact<
  "journey",
  "journeys",
  {
    startedAt?: string;
    endedAt?: string;
    origin?: string;
    destination?: string;
    distanceMeters?: number;
    points?: Array<{ latitude: number; longitude: number; recordedAt?: string }>;
  }
>;

export type InstalledApplicationArtifact = NodeArtifact<
  "installed_application",
  "applications",
  {
    name?: string;
    packageName?: string;
    version?: string;
    vendor?: string;
    installedAt?: string;
  }
>;

export type CookieArtifact = NodeArtifact<
  "cookie",
  "browser",
  {
    host?: string;
    name?: string;
    valuePreview?: string;
    path?: string;
    createdAt?: string;
    expiresAt?: string;
  }
>;

export type ApplicationUsageArtifact = NodeArtifact<
  "application_usage",
  "applications",
  {
    application?: string;
    packageName?: string;
    startedAt?: string;
    endedAt?: string;
    durationSeconds?: number;
    eventType?: string;
  }
>;

export type VisitedPageArtifact = NodeArtifact<
  "visited_page",
  "browser",
  {
    url?: string;
    title?: string;
    visitedAt?: string;
    browser?: string;
    visitCount?: number;
  }
>;

export type DictionaryWordArtifact = NodeArtifact<
  "dictionary_word",
  "search",
  {
    word?: string;
    language?: string;
    learnedAt?: string;
    usageCount?: number;
  }
>;

export type WebBookmarkArtifact = NodeArtifact<
  "web_bookmark",
  "browser",
  {
    url?: string;
    title?: string;
    folder?: string;
    createdAt?: string;
    browser?: string;
  }
>;

export type SharedFileArtifact = NodeArtifact<
  "shared_file",
  "files",
  {
    path?: string;
    name?: string;
    sharedWith?: string[];
    sharedAt?: string;
    service?: string;
  }
>;

export type BluetoothDeviceArtifact = NodeArtifact<
  "bluetooth_device",
  "networks",
  {
    name?: string;
    address?: string;
    pairedAt?: string;
    lastConnectedAt?: string;
    deviceClass?: string;
  }
>;

export type MapArtifact = NodeArtifact<
  "map",
  "maps",
  {
    name?: string;
    provider?: string;
    centerLatitude?: number;
    centerLongitude?: number;
    bounds?: unknown;
  }
>;

export type SearchedItemArtifact = NodeArtifact<
  "searched_item",
  "search",
  {
    query?: string;
    searchedAt?: string;
    application?: string;
    url?: string;
  }
>;

export type WirelessNetworkArtifact = NodeArtifact<
  "wireless_network",
  "networks",
  {
    ssid?: string;
    bssid?: string;
    security?: string;
    connectedAt?: string;
    lastConnectedAt?: string;
  }
>;

export type NotificationArtifact = NodeArtifact<
  "notification",
  "system",
  {
    application?: string;
    title?: string;
    body?: string;
    receivedAt?: string;
    action?: string;
  }
>;

export type CarvedStringArtifact = NodeArtifact<
  "carved_string",
  "search",
  {
    value?: string;
    encoding?: string;
    offset?: number;
    length?: number;
  }
>;

export type PoweringEventArtifact = NodeArtifact<
  "powering_event",
  "system",
  {
    eventType?: "power_on" | "power_off" | "reboot" | "sleep" | "wake" | "unknown";
    occurredAt?: string;
    sourceApp?: string;
  }
>;

/** Fallback model for plugin-specific records that do not fit a known model. */
export type GenericArtifact = BaseArtifact & {
  kind: "record";
  category: "other";
  fields: Record<string, unknown>;
};

export type CustomTableColumn = {
  key: string;
  label: string;
};

export type CustomTablePayload = {
  name: string;
  columns: CustomTableColumn[];
  rows: Record<string, unknown>[];
};

export type CustomTableArtifact = BaseArtifact & {
  kind: "custom_table";
  category: ArtifactCategory;
  table: CustomTablePayload;
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
  | SmsArtifact
  | EmailArtifact
  | MmsArtifact
  | ChatArtifact
  | InstantMessageArtifact
  | UserAccountArtifact
  | VoiceMailArtifact
  | CalendarEntryArtifact
  | PasswordArtifact
  | JourneyArtifact
  | InstalledApplicationArtifact
  | CookieArtifact
  | ApplicationUsageArtifact
  | VisitedPageArtifact
  | DictionaryWordArtifact
  | WebBookmarkArtifact
  | SharedFileArtifact
  | BluetoothDeviceArtifact
  | MapArtifact
  | SearchedItemArtifact
  | WirelessNetworkArtifact
  | NotificationArtifact
  | CarvedStringArtifact
  | PoweringEventArtifact
  | GenericArtifact
  | CustomTableArtifact;

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
