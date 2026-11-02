import type { ArtifactModelDefinition } from "./types";

type ModelDefinition = Omit<ArtifactModelDefinition, "schemaVersion" | "navigationCategory">;
export const standardArtifactModelDefinitions: ModelDefinition[] = [
  {
    kind: "refined_result", category: "search", label: "Refined Result", description: "A derived finding with its rationale and links to supporting artifacts.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "finding", label: "Finding", type: "string", required: true },
      { key: "summary", label: "Summary", type: "string" },
      { key: "rationale", label: "Rationale", type: "string" },
      { key: "relatedArtifactIds", label: "Supporting artifacts", type: "string[]" },
      { key: "ruleId", label: "Rule", type: "string" },
      { key: "matchedAt", label: "Matched", type: "datetime" },
    ],
  },
  {
    kind: "social_profile", category: "social", label: "Social Profile", description: "A social platform profile and its account identifiers.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "platform", label: "Platform", type: "string", required: true },
      { key: "username", label: "Username", type: "string", required: true },
      { key: "profileId", label: "Profile ID", type: "string" },
      { key: "displayName", label: "Display name", type: "string" },
      { key: "url", label: "URL", type: "string" },
      { key: "createdAt", label: "Created", type: "datetime" },
      { key: "lastActiveAt", label: "Last active", type: "datetime" },
    ],
  },
  {
    kind: "social_post", category: "social", label: "Social Post", description: "A social platform post with author, timestamps, and attachment references.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "platform", label: "Platform", type: "string", required: true },
      { key: "postId", label: "Post ID", type: "string" },
      { key: "author", label: "Author", type: "string" },
      { key: "authorId", label: "Author ID", type: "string" },
      { key: "body", label: "Body", type: "string" },
      { key: "url", label: "URL", type: "string" },
      { key: "postedAt", label: "Posted", type: "datetime" },
      { key: "editedAt", label: "Edited", type: "datetime" },
      { key: "attachments", label: "Attachments", type: "string[]" },
      { key: "replyToId", label: "Reply to", type: "string" },
      { key: "likeCount", label: "Likes", type: "number" },
      { key: "shareCount", label: "Shares", type: "number" },
    ],
  },
  {
    kind: "document", category: "files", label: "Document", description: "A document reference with authorship, file metadata, and timestamps.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "path", label: "Path", type: "string", required: true },
      { key: "name", label: "Name", type: "string" },
      { key: "title", label: "Title", type: "string" },
      { key: "author", label: "Author", type: "string" },
      { key: "mimeType", label: "MIME type", type: "string" },
      { key: "size", label: "Size", type: "number" },
      { key: "sha256", label: "SHA-256", type: "string" },
      { key: "pageCount", label: "Pages", type: "number" },
      { key: "createdAt", label: "Created", type: "datetime" },
      { key: "modifiedAt", label: "Modified", type: "datetime" },
    ],
  },
  {
    kind: "encrypted_item", category: "credentials", label: "Encrypted Item", description: "Encrypted file or container metadata without storing raw secrets.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "path", label: "Path", type: "string", required: true },
      { key: "encryptionType", label: "Encryption type", type: "string" },
      { key: "algorithm", label: "Algorithm", type: "string" },
      { key: "keyIdentifier", label: "Key identifier", type: "string" },
      { key: "provider", label: "Provider", type: "string" },
      { key: "isLocked", label: "Locked", type: "boolean" },
    ],
  },
  {
    kind: "connected_device", category: "networks", label: "Connected Device", description: "A connected peripheral or device and its connection history.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "name", label: "Name", type: "string", required: true },
      { key: "deviceType", label: "Device type", type: "string" },
      { key: "identifier", label: "Identifier", type: "string" },
      { key: "manufacturer", label: "Manufacturer", type: "string" },
      { key: "model", label: "Model", type: "string" },
      { key: "serialNumber", label: "Serial number", type: "string" },
      { key: "connectionType", label: "Connection type", type: "string" },
      { key: "firstConnectedAt", label: "First connected", type: "datetime" },
      { key: "lastConnectedAt", label: "Last connected", type: "datetime" },
    ],
  },
  {
    kind: "custom_table", category: "other", label: "Custom Table", description: "A plugin-defined table with named columns and rows.",
    fields: [
      { key: "label", label: "Label", type: "string", required: true },
      { key: "table", label: "Table", type: "json", required: true },
    ],
  },
];
