import type { Artifact } from "./types";

type CategoryDefinition = {
  id: string;
  label: string;
  description: string;
  icon: string;
  primaryKind: Artifact["kind"];
  modelKinds: readonly Artifact["kind"][];
};

/** Stable category IDs and model ownership. Display labels are not payload categories. */
export const artifactCategoryDefinitions = [
  { id: "refined_results", label: "Refined Results", description: "Correlated findings, search results, and timeline events.", icon: "search", primaryKind: "refined_result", modelKinds: ["refined_result", "timeline_event", "dictionary_word", "searched_item", "carved_string"] },
  { id: "web_related", label: "Web Related", description: "Web history, bookmarks, cookies, and visited pages.", icon: "globe", primaryKind: "browser_history", modelKinds: ["browser_history", "visited_page", "cookie", "web_bookmark"] },
  { id: "communication", label: "Communication", description: "Calls, contacts, conversations, and messages.", icon: "message-square", primaryKind: "message", modelKinds: ["call", "contact", "message", "sms", "mms", "chat", "instant_message", "voice_mail"] },
  { id: "social_networking", label: "Social Networking", description: "Social platform profiles and posts.", icon: "users", primaryKind: "social_profile", modelKinds: ["social_profile", "social_post"] },
  { id: "media", label: "Media", description: "Image, video, and audio references with media metadata.", icon: "image", primaryKind: "media", modelKinds: ["media"] },
  { id: "email_calendar", label: "Email & Calendar", description: "Email messages, calendar appointments, and reminders.", icon: "mail", primaryKind: "email", modelKinds: ["email", "calendar_entry"] },
  { id: "documents", label: "Documents", description: "Documents, notes, file metadata, and shared files.", icon: "file-text", primaryKind: "document", modelKinds: ["document", "file", "note", "shared_file"] },
  { id: "application_usage", label: "Application Usage", description: "Installed applications, packages, and usage events.", icon: "app-window", primaryKind: "application_usage", modelKinds: ["application", "installed_application", "application_usage"] },
  { id: "operating_system", label: "Operating System", description: "System configuration, accounts, notifications, and power events.", icon: "monitor", primaryKind: "system", modelKinds: ["account", "user_account", "system", "notification", "powering_event"] },
  { id: "encryption_credentials", label: "Encryption & Credentials", description: "Credential indicators and encrypted evidence metadata.", icon: "key-round", primaryKind: "credential", modelKinds: ["credential", "password", "encrypted_item"] },
  { id: "connected_devices", label: "Connected Devices", description: "Connected devices, Bluetooth pairings, and wireless networks.", icon: "usb", primaryKind: "connected_device", modelKinds: ["connected_device", "bluetooth_device", "wireless_network"] },
  { id: "location_travel", label: "Location & Travel", description: "Recorded coordinates, journeys, and map references.", icon: "map-pin", primaryKind: "location", modelKinds: ["location", "journey", "map"] },
  { id: "custom", label: "Custom", description: "Plugin-specific records and custom tables.", icon: "table-2", primaryKind: "record", modelKinds: ["record", "custom_table"] },
] as const satisfies readonly CategoryDefinition[];

export type ArtifactCategoryId = (typeof artifactCategoryDefinitions)[number]["id"];
export type ArtifactNavigationCategory = (typeof artifactCategoryDefinitions)[number]["label"];
export type ArtifactCategoryDefinition = (typeof artifactCategoryDefinitions)[number];

export const artifactCategoryByKind = new Map<string, ArtifactCategoryDefinition>(
  artifactCategoryDefinitions.flatMap((category) => category.modelKinds.map((kind) => [kind, category] as const)),
);

/** Typed payload union for each main category, including its specialized models. */
export type ArtifactByCategory = {
  [Category in ArtifactCategoryDefinition as Category["id"]]: Extract<Artifact, { kind: Category["modelKinds"][number] }>;
};
