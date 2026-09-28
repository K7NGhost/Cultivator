import { extractDateRange, type AssistantDateRange } from "./assistantDateRange";
import { blankSpans } from "./textSpans";

export type AssistantTarget = "messages" | "photos" | "videos" | "calls" | "contacts";

export type AssistantPerson = {
  name: string;
  /** Digits only, e.g. "15550177". */
  phones: string[];
  /** Lowercased words that refer to this person in a typed question. */
  aliases: string[];
};

export type { AssistantDateRange, YearlessWindow } from "./assistantDateRange";
export { formatDateRangeLabel } from "./assistantDateRange";

export type AssistantQuery = {
  text: string;
  /** Empty means every artifact type. */
  targets: AssistantTarget[];
  people: {
    with: AssistantPerson[];
    from: AssistantPerson[];
    to: AssistantPerson[];
  };
  dateRange: AssistantDateRange | null;
  keywords: string[];
  notes: string[];
};

export type ParseOptions = {
  people: AssistantPerson[];
  now?: Date;
};

const TARGET_WORDS: Record<string, AssistantTarget> = {
  message: "messages", messages: "messages", msg: "messages", msgs: "messages",
  text: "messages", texts: "messages", texted: "messages", texting: "messages",
  sms: "messages", chat: "messages", chats: "messages", conversation: "messages",
  conversations: "messages", convo: "messages", convos: "messages", said: "messages",
  say: "messages", says: "messages", saying: "messages", wrote: "messages",
  photo: "photos", photos: "photos", picture: "photos", pictures: "photos",
  pic: "photos", pics: "photos", image: "photos", images: "photos", selfie: "photos",
  selfies: "photos", screenshot: "photos", screenshots: "photos",
  video: "videos", videos: "videos", clip: "videos", clips: "videos",
  footage: "videos", recording: "videos", recordings: "videos",
  call: "calls", calls: "calls", called: "calls", calling: "calls", rang: "calls",
  contact: "contacts", contacts: "contacts",
};

/** Once one of these appears, later target words are search content instead. */
const CONTENT_CUES = new Set([
  "about", "mention", "mentions", "mentioning", "mentioned", "containing", "contains",
  "contain", "saying", "says", "said", "say", "word", "words", "phrase", "regarding",
]);

const STOPWORDS = new Set([
  "a", "about", "after", "all", "am", "among", "amongst", "an", "and", "any", "anyone",
  "anything", "are", "as", "at", "be", "been", "before", "beginning", "between", "bring",
  "by", "can", "communicate", "communicated", "communication", "communications", "contain",
  "containing", "contains", "could", "did", "discuss", "discussed", "display", "do", "does",
  "during", "each", "earlier", "entire", "every", "everyone", "everything", "fetch", "find",
  "for", "from", "get", "give", "had", "has", "have", "her", "here", "him", "his", "how",
  "i", "in", "include", "includes", "including", "into", "involving", "is", "it", "its",
  "just", "later", "let", "like", "list", "look", "made", "may", "me", "mention", "mentioned",
  "mentioning", "mentions", "might", "must", "my", "need", "of", "on", "only", "open", "or", "our",
  "people", "person", "phrase", "please", "prior", "pull", "received", "regarding",
  "related", "s", "said", "say", "saying", "says", "search", "see", "sent", "shall", "should", "show",
  "showing", "shows", "since", "some", "speak", "spoke", "spoken", "starting", "taken",
  "talk", "talked", "talking", "than", "that", "the", "their", "them", "there", "these",
  "they", "this", "those", "through", "thru", "till", "to", "until", "up", "via", "want",
  "was", "were", "what", "when", "where", "which", "who", "whole", "whom", "will", "with",
  "within", "word", "words", "would", "wrote", "you", "your",
  // How examiners phrase questions, not what they ask about.
  "anybody", "attempt", "attempted", "attempting", "being", "everybody", "getting", "instances",
  "ppl", "somebody", "someone", "stuff", "thing", "things", "tried", "tries", "try", "trying",
]);

/**
 * Turns a free-text question into a structured, reviewable filter. The
 * interpretation is deterministic and rule-based on purpose: every result
 * shown to an examiner must trace back to a filter they can see and edit.
 */
export function parseAssistantQuery(text: string, options: ParseOptions): AssistantQuery {
  const now = options.now ?? new Date();
  const notes: string[] = [];
  const keywords: string[] = [];

  let working = text.replace(/[–—]/g, "-");
  working = extractQuotedPhrases(working, keywords);
  working = ` ${working.toLowerCase().replace(/\s+/g, " ").trim()} `
    .replace(/\bphone (calls?)\b/g, "$1")
    .replace(/\baddress book\b/g, "contacts");

  // Known names are never "corrected" into months ("marc" is not March).
  const protectedWords = new Set(options.people.flatMap((person) => person.aliases));
  const dateResult = extractDateRange(working, now, protectedWords);
  working = dateResult.text;
  notes.push(...dateResult.notes);

  const peopleResult = extractPeople(working, options.people, notes);
  working = peopleResult.text;

  const targetResult = extractTargets(working);
  working = targetResult.text;

  keywords.push(...extractKeywords(working));

  return {
    text,
    targets: targetResult.targets,
    people: peopleResult.people,
    dateRange: dateResult.range,
    keywords: Array.from(new Set(keywords.filter(Boolean))),
    notes,
  };
}

/** Builds the person directory the parser recognizes names against. */
export function buildPersonDirectory(
  entries: Array<{ name?: string | null; phones?: Array<string | null | undefined> }>,
): AssistantPerson[] {
  const byKey = new Map<string, AssistantPerson>();

  for (const entry of entries) {
    const phones = (entry.phones ?? [])
      .map((phone) => digitsOf(phone ?? ""))
      .filter((digits) => digits.length >= 7);
    const name = (entry.name ?? "").trim();

    if (!name && phones.length === 0) {
      continue;
    }

    const existing =
      phones.map((phone) => byKey.get(`phone:${phone}`)).find(Boolean) ??
      (name ? byKey.get(`name:${name.toLowerCase()}`) : undefined);
    const person: AssistantPerson = existing ?? {
      name: name || (entry.phones ?? []).find(Boolean) || phones[0],
      phones: [],
      aliases: [],
    };

    if (existing && name && looksLikePhone(existing.name)) {
      person.name = name;
    }

    for (const phone of phones) {
      if (!person.phones.includes(phone)) {
        person.phones.push(phone);
      }
      byKey.set(`phone:${phone}`, person);
    }

    if (name) {
      byKey.set(`name:${name.toLowerCase()}`, person);
      for (const alias of nameAliases(name)) {
        if (!person.aliases.includes(alias)) {
          person.aliases.push(alias);
        }
      }
    }
  }

  return Array.from(new Set(byKey.values()));
}

/** Splits "Jane Miller (+1-555-0177)" into its name and number. */
export function splitParticipant(value: string): { name: string; phone: string } {
  const match = value.match(/^(.*?)\s*\(([^()]*\d[^()]*)\)\s*$/);

  if (match) {
    return { name: match[1].trim(), phone: match[2].trim() };
  }

  return looksLikePhone(value)
    ? { name: "", phone: value.trim() }
    : { name: value.trim(), phone: "" };
}

export function digitsOf(value: string) {
  return value.replace(/\D/g, "");
}

function looksLikePhone(value: string) {
  return /^\+?[\d\s().-]{7,}$/.test(value.trim()) && digitsOf(value).length >= 7;
}

function nameAliases(name: string) {
  const base = name.toLowerCase().replace(/\([^)]*\)/g, " ").replace(/\s+/g, " ").trim();
  const aliases = new Set<string>([name.toLowerCase().trim()]);

  if (base) {
    aliases.add(base);
    for (const part of base.split(" ")) {
      aliases.add(part);
    }
  }

  return Array.from(aliases).filter((alias) => alias.length > 1 && !STOPWORDS.has(alias));
}

function extractQuotedPhrases(text: string, keywords: string[]) {
  return text.replace(
    /"([^"]+)"|“([^”]+)”|(?:^|\s)'([^']+)'(?=\s|$)/g,
    (match, double, curly, single) => {
      const phrase = (double ?? curly ?? single ?? "").trim().toLowerCase();
      if (phrase) {
        keywords.push(phrase);
      }
      return " ".repeat(match.length);
    },
  );
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function extractPeople(text: string, directory: AssistantPerson[], notes: string[]) {
  const people = {
    with: [] as AssistantPerson[],
    from: [] as AssistantPerson[],
    to: [] as AssistantPerson[],
  };
  const seen = new Set<AssistantPerson>();

  const add = (person: AssistantPerson, role: "with" | "from" | "to") => {
    if (!seen.has(person)) {
      seen.add(person);
      people[role].push(person);
    }
  };

  const roleBefore = (index: number): "with" | "from" | "to" => {
    const before = text.slice(0, index);
    if (/\b(from|by)\s+$/.test(before)) return "from";
    if (/\b(to|for)\s+$/.test(before)) return "to";
    return "with";
  };

  for (const match of text.matchAll(/\+?\d[\d\s().-]{5,}\d/g)) {
    const digits = digitsOf(match[0]);
    if (digits.length < 7) {
      continue;
    }
    const index = match.index ?? 0;
    const role = roleBefore(index);
    const matches = directory.filter((person) =>
      person.phones.some((phone) => phone.endsWith(digits) || digits.endsWith(phone)),
    );
    if (matches.length > 0) {
      matches.forEach((person) => add(person, role));
    } else {
      add({ name: match[0].trim(), phones: [digits], aliases: [] }, role);
    }
    text = blankSpans(text, [[index, index + match[0].length]]);
  }

  const peopleByAlias = new Map<string, AssistantPerson[]>();
  for (const person of directory) {
    for (const alias of person.aliases) {
      peopleByAlias.set(alias, [...(peopleByAlias.get(alias) ?? []), person]);
    }
  }

  // Longest aliases first, so "jane miller" wins over "jane".
  const aliases = Array.from(peopleByAlias.keys()).sort((left, right) => right.length - left.length);

  for (const alias of aliases) {
    const pattern = new RegExp(String.raw`\b${escapeRegExp(alias)}\b`, "g");
    for (const match of Array.from(text.matchAll(pattern))) {
      const index = match.index ?? 0;
      const matched = peopleByAlias.get(alias) ?? [];
      const role = roleBefore(index);
      matched.forEach((person) => add(person, role));
      if (matched.length > 1) {
        notes.push(
          `"${alias}" matches ${matched.length} people: ${matched.map((person) => person.name).join(", ")}.`,
        );
      }
      text = blankSpans(text, [[index, index + match[0].length]]);
    }
  }

  return { text, people };
}

function extractTargets(text: string) {
  const targets: AssistantTarget[] = [];
  let contentMode = false;

  const rebuilt = text.replace(/[a-z]+/g, (word) => {
    const target = TARGET_WORDS[word];
    const isCue = CONTENT_CUES.has(word);

    if (target && !contentMode) {
      if (!targets.includes(target)) {
        targets.push(target);
      }
      contentMode ||= isCue;
      return " ".repeat(word.length);
    }

    contentMode ||= isCue;
    return word;
  });

  return { text: rebuilt, targets };
}

function extractKeywords(text: string) {
  return (text.match(/[a-z0-9$][a-z0-9$,.'-]*/g) ?? [])
    .map((token) => token.replace(/[,.'-]+$/, "").replace(/'s$/, ""))
    .filter((token) => token.length > 1 || /\d/.test(token))
    .filter((token) => !STOPWORDS.has(token));
}
