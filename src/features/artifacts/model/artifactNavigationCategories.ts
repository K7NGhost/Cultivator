import { artifactCategoryByKind, artifactCategoryDefinitions, type ArtifactNavigationCategory } from "./artifactCategoryDefinitions";
import type { StoredArtifactRecord } from "./types";

export type { ArtifactNavigationCategory } from "./artifactCategoryDefinitions";
export const ARTIFACT_NAVIGATION_CATEGORIES = artifactCategoryDefinitions.map((category) => category.label);

function normalize(value: string) {
  return value.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();
}

const categoryAliases = new Map<string, ArtifactNavigationCategory>(
  artifactCategoryDefinitions.flatMap((category) => [
    [normalize(category.id), category.label],
    [normalize(category.label), category.label],
  ] as [string, ArtifactNavigationCategory][]),
);

for (const [category, aliases] of [
  ["Web Related", ["browser"]],
  ["Communication", ["calls", "contacts", "messages", "communications", "chats"]],
  ["Social Networking", ["social"]],
  ["Email & Calendar", ["email", "calendar"]],
  ["Documents", ["files", "notes"]],
  ["Application Usage", ["applications"]],
  ["Operating System", ["accounts", "system"]],
  ["Encryption & Credentials", ["credentials"]],
  ["Connected Devices", ["networks"]],
  ["Location & Travel", ["locations", "journeys", "maps"]],
  ["Refined Results", ["search", "timeline"]],
] satisfies [ArtifactNavigationCategory, string[]][]) {
  for (const alias of aliases) categoryAliases.set(alias, category);
}

// Use artifact descriptions before app-family hints: e.g. Instagram Messages
// belongs to Communication, while Instagram Posts belongs to Social Networking.
const contentRules: readonly [ArtifactNavigationCategory, RegExp][] = [
  ["Encryption & Credentials", /\b(passwords?|credentials?|keychain|encrypted|encryption|authenticator)\b/],
  ["Email & Calendar", /\b(e ?mails?|calendar|appointments?|reminders?|gmail|protonmail|proton mail|outlook|yahoo mail|apple mail|thunderbird)\b/],
  ["Communication", /\b(sms|mms|imessage|rcs|messages?|messaging|chats?|conversations?|contacts?|calls?|voicemails?|voice mail)\b/],
  ["Web Related", /\b(browsers?|bookmarks?|cookies?|web history|visited pages?|browser history)\b/],
  ["Location & Travel", /\b(locations?|geolocation|geo location|gps|gnss|journeys?|trips?|navigation|travel|coordinates?|routes?)\b/],
  ["Connected Devices", /\b(bluetooth|wi ?fi|wireless|connected devices?|device connections?|pairings?|usb|dhcp)\b/],
  ["Media", /\b(photos?|images?|videos?|audio|music|media|gallery|albums?|podcasts?|exif)\b/],
  ["Documents", /\b(documents?|notes?|files?|file ?system|downloads?|clipboard|pdf)\b/],
  ["Social Networking", /\b(social|posts?|tweets?|followers?|following|friends?)\b/],
  ["Application Usage", /\b(installed apps?|installed applications?|app inventory|app file inventory|app usage|application usage|app interaction|usage stats|user activity|device usage|screen time|digital wellbeing)\b/],
  ["Operating System", /\b(system|sysdiagnose|device information|device users?|accounts?|identifiers?|permissions?|notifications?|battery|powerlog|power log|power events?|power telemetry|powering|os updates?|software updates?|ios build|settings|preferences|diagnostic|diagnostics|backup|sim info|alarms?|timers?|world ?clock|control center|spring ?board|home screen|mobile activation|identity lookup service)\b/],
  ["Refined Results", /\b(timeline|search results?|spotlight searches|carved strings?|database journal strings|dictionary)\b/],
];

const familyRules: readonly [ArtifactNavigationCategory, RegExp][] = [
  ["Web Related", /\b(safari|chromium|chrome|firefox|duckduckgo|duck duck go|brave|opera|ornet|or net|tor browser|onion browser)\b/],
  ["Email & Calendar", /\b(gmail|protonmail|proton mail|outlook|yahoo mail|apple mail|thunderbird|fair ?email|k 9 mail|telekom mail|todoist)\b/],
  ["Communication", /\b(whats ?app|telegram|signal|facebook messenger|google voice|google duo|google chat|discord|slack|teams|skype|viber|wickr|threema|wechat|we chat|kik|kakaotalk|kakao talk|groupme|group me|textnow|text now|teleguard|simplex|simple x|session messenger|silent phone|wire messenger|zoom|facetime|face time)\b/],
  ["Social Networking", /\b(facebook|instagram|snapchat|snap chat|tiktok|tik tok|twitter|reddit|pinterest|linkedin|linked in|mastodon|bluesky|be ?real|me ?we|threads|tinder|bumble|grindr|truth social|weibo|foursquare swarm)\b/],
  ["Location & Travel", /\b(life360|life 360|waze|google maps|apple maps|find my|airtags?|air tags?|tile app|uber|booking com|airbnb|ryanair|united airlines|citymapper|city mapper|what3words|what3 words|organic maps|osmand|here wego|here we go)\b/],
  ["Media", /\b(spotify|netflix|prime video|disney|shazam|vlc|snapseed|capcut|cap cut|newpipe|new pipe|musicolet|poweramp|antenna ?pod)\b/],
  ["Documents", /\b(google drive|one ?drive|dropbox|drop box|yandex disk|proton drive|cloud storage|joplin|notion|google keep|google docs|google sheets|adobe acrobat|solid explorer)\b/],
  ["Encryption & Credentials", /\b(bitwarden|bit warden|keepassdx|keepass dx|aegis|knox secure folder|galleryvault|gallery vault|keepsafe|keep safe|vaulty|private photo vault)\b/],
  ["Connected Devices", /\b(core accessories|google cast|trusted peers)\b/],
  ["Operating System", /\b(clock|ios screens)\b/],
  ["Application Usage", /\b(biome|knowledge ?c|interaction ?c|app store|play store|mobile installation|app permissions|app roles|health|fitness|fitbit|garmin|oura ring|chatgpt|chat gpt|claude|network usage|crashlytics)\b/],
];

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function inferCategory(value: string, rules: readonly [ArtifactNavigationCategory, RegExp][]) {
  const normalized = normalize(value);
  return rules.find(([, pattern]) => pattern.test(normalized))?.[0];
}

function getArtifactKind(artifact: StoredArtifactRecord) {
  const payload = artifact.payload;

  if (
    payload &&
    typeof payload === "object" &&
    !Array.isArray(payload) &&
    typeof payload.kind === "string"
  ) {
    return payload.kind;
  }

  return artifact.resultKind;
}

export function getArtifactNavigationCategory(artifact: StoredArtifactRecord): ArtifactNavigationCategory {
  const knownCategory = artifactCategoryByKind.get(getArtifactKind(artifact))?.label;
  if (knownCategory && knownCategory !== "Custom") return knownCategory;

  const payload = object(artifact.payload);
  const explicitCategory = categoryAliases.get(normalize(text(payload.category)));
  if (explicitCategory && explicitCategory !== "Custom") return explicitCategory;

  const parser = object(payload.parser);
  const definition = object(parser.artifact);
  const table = object(payload.table);
  const group = object(payload.group);
  // Only metadata is inspected. Row values, source paths and evidence text must
  // never determine the navigation category or mutate the stored artifact.
  const names = [definition.name, table.name, payload.label, artifact.label].map(text).filter(Boolean);
  for (const name of names) {
    const inferred = inferCategory(name, contentRules);
    if (inferred) return inferred;
  }
  for (const hint of [parser.category, definition.category, group.label, ...names]) {
    const value = text(hint);
    const alias = categoryAliases.get(normalize(value));
    if (alias && alias !== "Custom") return alias;
    const inferred = inferCategory(value, contentRules) ?? inferCategory(value, familyRules);
    if (inferred) return inferred;
  }
  return "Custom";
}
