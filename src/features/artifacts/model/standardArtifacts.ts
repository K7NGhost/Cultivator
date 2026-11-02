import type { BaseArtifact } from "./types";

export type RefinedResultArtifact = BaseArtifact & {
  kind: "refined_result";
  category: "search";
  finding: string;
  summary?: string;
  rationale?: string;
  relatedArtifactIds?: string[];
  ruleId?: string;
  matchedAt?: string;
};

export type SocialProfileArtifact = BaseArtifact & {
  kind: "social_profile";
  category: "social";
  platform: string;
  username: string;
  profileId?: string;
  displayName?: string;
  url?: string;
  createdAt?: string;
  lastActiveAt?: string;
};

export type SocialPostArtifact = BaseArtifact & {
  kind: "social_post";
  category: "social";
  platform: string;
  postId?: string;
  author?: string;
  authorId?: string;
  body?: string;
  url?: string;
  postedAt?: string;
  editedAt?: string;
  attachments?: string[];
  replyToId?: string;
  likeCount?: number;
  shareCount?: number;
};

export type DocumentArtifact = BaseArtifact & {
  kind: "document";
  category: "files";
  path: string;
  name?: string;
  title?: string;
  author?: string;
  mimeType?: string;
  size?: number;
  sha256?: string;
  pageCount?: number;
  createdAt?: string;
  modifiedAt?: string;
};

export type EncryptedItemArtifact = BaseArtifact & {
  kind: "encrypted_item";
  category: "credentials";
  path: string;
  encryptionType?: string;
  algorithm?: string;
  keyIdentifier?: string;
  provider?: string;
  isLocked?: boolean;
};

export type ConnectedDeviceArtifact = BaseArtifact & {
  kind: "connected_device";
  category: "networks";
  name: string;
  deviceType?: string;
  identifier?: string;
  manufacturer?: string;
  model?: string;
  serialNumber?: string;
  connectionType?: string;
  firstConnectedAt?: string;
  lastConnectedAt?: string;
};
