export type AssistantTarget = "messages" | "photos" | "videos" | "calls" | "contacts";

export type AssistantPerson = {
  name: string;
  /** Digits only, e.g. "15550177". */
  phones: string[];
  /** Lowercased words that refer to this person in a typed question. */
  aliases: string[];
};

export type AssistantDateRange = {
  start: Date | null;
  end: Date | null;
  /** Year-agnostic month window, e.g. "in April" with no year given. */
  monthsOfYear?: { from: number; to: number };
  label: string;
};

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

type Span = [number, number];

type DateMatch = {
  spans: Span[];
  start: Date | null;
  end: Date | null;
  monthsOfYear?: { from: number; to: number };
  label?: string;
};

const MONTHS: Record<string, number> = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7, sep: 8, sept: 8,
  september: 8, oct: 9, october: 9, nov: 10, november: 10, dec: 11, december: 11,
};

const MONTH_NAMES = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

const MONTH = String.raw`\b(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sept?(?:ember)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\b\.?`;
const DAY = String.raw`(\d{1,2})(?!\d)(?:st|nd|rd|th)?`;
const YEAR = String.raw`((?:19|20)\d{2})(?![\d/-])`;
/** A year that isn't part of a phone number or numeric date. */
const STANDALONE_YEAR = String.raw`(?<![\d+/-])((?:19|20)\d{2})(?![\d/-])`;
const RANGE_SEPARATOR = String.raw`\s*(?:-|to|through|thru|until|till|and)\s*`;

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
]);

const DATE_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
});

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

  const dateResult = extractDateRange(working, now);
  working = dateResult.text;

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

export function formatDateRangeLabel(start: Date | null, end: Date | null) {
  if (start && end) {
    return `${DATE_FORMAT.format(start)} – ${DATE_FORMAT.format(end)}`;
  }

  if (start) {
    return `On or after ${DATE_FORMAT.format(start)}`;
  }

  return end ? `On or before ${DATE_FORMAT.format(end)}` : "Any date";
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

function blank(text: string, spans: Span[]) {
  let result = text;
  for (const [start, end] of spans) {
    result = result.slice(0, start) + " ".repeat(end - start) + result.slice(end);
  }
  return result;
}

function monthIndex(token: string) {
  const cleaned = token.replace(/\.$/, "");
  return MONTHS[cleaned] ?? MONTHS[cleaned.slice(0, 3)];
}

function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function endOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate(), 23, 59, 59, 999);
}

function endOfMonth(year: number, month: number) {
  return new Date(year, month + 1, 0, 23, 59, 59, 999);
}

function validDay(year: number, month: number, day: number) {
  const date = new Date(year, month, day);
  return date.getMonth() === month && date.getDate() === day ? date : null;
}

function extractDateRange(text: string, now: Date) {
  const match =
    matchIsoRange(text) ??
    matchMonthRange(text) ??
    matchSpecificDay(text) ??
    matchMonth(text) ??
    matchYear(text) ??
    matchRelative(text, now);

  if (!match) {
    return { text, range: null as AssistantDateRange | null };
  }

  let { start, end } = match;
  const spans = [...match.spans];
  const firstStart = Math.min(...spans.map(([spanStart]) => spanStart));
  const modifier = match.monthsOfYear
    ? null
    : text
        .slice(0, firstStart)
        .match(/\b(before|prior to|earlier than|until|till|up to|through|after|later than|since|starting|beginning)\s+$/);

  if (modifier) {
    const word = modifier[1];
    spans.push([firstStart - modifier[0].length, firstStart]);

    if (["before", "prior to", "earlier than"].includes(word)) {
      end = start ? new Date(start.getTime() - 1) : end;
      start = null;
    } else if (["until", "till", "up to", "through"].includes(word)) {
      start = null;
    } else if (["after", "later than"].includes(word)) {
      start = end ? new Date(end.getTime() + 1) : start;
      end = null;
    } else {
      end = null;
    }
  }

  return {
    text: blank(text, spans),
    range: {
      start,
      end,
      monthsOfYear: match.monthsOfYear,
      label: match.monthsOfYear ? match.label ?? "Any year" : formatDateRangeLabel(start, end),
    } as AssistantDateRange,
  };
}

function findStandaloneYear(text: string, exclude: Span) {
  for (const match of text.matchAll(new RegExp(STANDALONE_YEAR, "g"))) {
    const index = match.index ?? 0;
    if (index >= exclude[0] && index < exclude[1]) {
      continue;
    }
    return { year: Number(match[1]), span: [index, index + match[0].length] as Span };
  }
  return null;
}

/** Absorbs a leading "between"/"from" so it isn't mistaken for anything else. */
function absorbLead(text: string, span: Span): Span {
  const lead = text.slice(0, span[0]).match(/\b(between|from)\s+$/);
  return lead ? [span[0] - lead[0].length, span[1]] : span;
}

function matchIsoRange(text: string): DateMatch | null {
  const match = text.match(
    /(\d{4})-(\d{1,2})-(\d{1,2})\s+(?:-|to|through|thru|until|till|and)\s+(\d{4})-(\d{1,2})-(\d{1,2})/,
  );
  if (!match || match.index === undefined) {
    return null;
  }
  const start = validDay(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  const end = validDay(Number(match[4]), Number(match[5]) - 1, Number(match[6]));
  if (!start || !end) {
    return null;
  }
  return {
    spans: [absorbLead(text, [match.index, match.index + match[0].length])],
    start,
    end: endOfDay(end),
  };
}

function matchMonthRange(text: string): DateMatch | null {
  const match = text.match(
    new RegExp(
      String.raw`${MONTH}(?:\s+${DAY})?(?:,?\s+${YEAR})?${RANGE_SEPARATOR}${MONTH}(?:\s+${DAY})?(?:,?\s+${YEAR})?`,
    ),
  );
  if (!match || match.index === undefined) {
    return null;
  }

  const month1 = monthIndex(match[1]);
  const day1 = match[2] ? Number(match[2]) : null;
  let year1 = match[3] ? Number(match[3]) : null;
  const month2 = monthIndex(match[4]);
  const day2 = match[5] ? Number(match[5]) : null;
  let year2 = match[6] ? Number(match[6]) : null;
  const span = absorbLead(text, [match.index, match.index + match[0].length]);
  const spans: Span[] = [span];

  if (year1 === null && year2 === null) {
    const standalone = findStandaloneYear(text, span);
    if (!standalone) {
      return {
        spans,
        start: null,
        end: null,
        monthsOfYear: { from: month1, to: month2 },
        label: `${MONTH_NAMES[month1]} – ${MONTH_NAMES[month2]} (any year)`,
      };
    }
    year2 = standalone.year;
    spans.push(standalone.span);
  }

  year1 ??= month1 <= month2 ? year2! : year2! - 1;
  year2 ??= month2 >= month1 ? year1 : year1 + 1;

  const start = day1 ? validDay(year1, month1, day1) : new Date(year1, month1, 1);
  const lastDay = day2 ? validDay(year2, month2, day2) : null;
  const end = day2 ? (lastDay ? endOfDay(lastDay) : null) : endOfMonth(year2, month2);

  return start && end ? { spans, start, end } : null;
}

function matchSpecificDay(text: string): DateMatch | null {
  const candidates: Array<{ span: Span; year: number | null; month: number; day: number }> = [];

  const monthFirst = text.match(new RegExp(String.raw`${MONTH}\s+${DAY}(?:,?\s+${YEAR})?`));
  if (monthFirst?.index !== undefined) {
    candidates.push({
      span: [monthFirst.index, monthFirst.index + monthFirst[0].length],
      month: monthIndex(monthFirst[1]),
      day: Number(monthFirst[2]),
      year: monthFirst[3] ? Number(monthFirst[3]) : null,
    });
  }

  const dayFirst = text.match(new RegExp(String.raw`\b${DAY}\s+(?:of\s+)?${MONTH}(?:,?\s+${YEAR})?`));
  if (dayFirst?.index !== undefined) {
    candidates.push({
      span: [dayFirst.index, dayFirst.index + dayFirst[0].length],
      day: Number(dayFirst[1]),
      month: monthIndex(dayFirst[2]),
      year: dayFirst[3] ? Number(dayFirst[3]) : null,
    });
  }

  const iso = text.match(/(?<![\d-])(\d{4})-(\d{1,2})-(\d{1,2})(?![\d-])/);
  if (iso?.index !== undefined) {
    candidates.push({
      span: [iso.index, iso.index + iso[0].length],
      year: Number(iso[1]),
      month: Number(iso[2]) - 1,
      day: Number(iso[3]),
    });
  }

  const us = text.match(/(?<![\d/])(\d{1,2})\/(\d{1,2})\/(\d{2,4})(?![\d/])/);
  if (us?.index !== undefined) {
    const year = Number(us[3]) < 100 ? 2000 + Number(us[3]) : Number(us[3]);
    candidates.push({
      span: [us.index, us.index + us[0].length],
      month: Number(us[1]) - 1,
      day: Number(us[2]),
      year,
    });
  }

  candidates.sort((left, right) => left.span[0] - right.span[0]);

  for (const candidate of candidates) {
    const spans: Span[] = [candidate.span];
    let year = candidate.year;

    if (year === null) {
      const standalone = findStandaloneYear(text, candidate.span);
      if (!standalone) {
        continue;
      }
      year = standalone.year;
      spans.push(standalone.span);
    }

    const date = validDay(year, candidate.month, candidate.day);
    if (date) {
      return { spans, start: startOfDay(date), end: endOfDay(date) };
    }
  }

  return null;
}

function matchMonth(text: string): DateMatch | null {
  const match = text.match(new RegExp(String.raw`${MONTH}(?:,?\s+(?:of\s+)?${YEAR})?`));
  if (!match || match.index === undefined) {
    return null;
  }

  // "may" is an ordinary word too; only read it as a month next to a year
  // or a date preposition.
  if (
    match[1] === "may" &&
    !match[2] &&
    !/\b(in|during|from|of|since|before|after|until)\s+$/.test(text.slice(0, match.index))
  ) {
    return null;
  }

  const month = monthIndex(match[1]);
  const span: Span = [match.index, match.index + match[0].length];
  const spans: Span[] = [span];
  let year = match[2] ? Number(match[2]) : null;

  if (year === null) {
    const standalone = findStandaloneYear(text, span);
    if (standalone) {
      year = standalone.year;
      spans.push(standalone.span);
    }
  }

  if (year === null) {
    return {
      spans,
      start: null,
      end: null,
      monthsOfYear: { from: month, to: month },
      label: `${MONTH_NAMES[month]} (any year)`,
    };
  }

  return { spans, start: new Date(year, month, 1), end: endOfMonth(year, month) };
}

function matchYear(text: string): DateMatch | null {
  const match = text.match(new RegExp(STANDALONE_YEAR));
  if (!match || match.index === undefined) {
    return null;
  }
  const year = Number(match[1]);
  return {
    spans: [[match.index, match.index + match[0].length]],
    start: new Date(year, 0, 1),
    end: endOfMonth(year, 11),
  };
}

function matchRelative(text: string, now: Date): DateMatch | null {
  const match = text.match(
    /\b(today|yesterday|this week|last week|this month|last month|this year|last year|(?:last|past) (\d+) (days?|weeks?|months?|years?))\b/,
  );
  if (!match || match.index === undefined) {
    return null;
  }

  const today = startOfDay(now);
  const phrase = match[1];
  let start: Date;
  let end: Date = endOfDay(now);

  if (phrase === "today") {
    start = today;
  } else if (phrase === "yesterday") {
    start = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
    end = endOfDay(start);
  } else if (phrase === "this week" || phrase === "last week") {
    const mondayOffset = (today.getDay() + 6) % 7;
    const thisMonday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - mondayOffset);
    if (phrase === "this week") {
      start = thisMonday;
    } else {
      start = new Date(thisMonday.getFullYear(), thisMonday.getMonth(), thisMonday.getDate() - 7);
      end = new Date(thisMonday.getTime() - 1);
    }
  } else if (phrase === "this month") {
    start = new Date(today.getFullYear(), today.getMonth(), 1);
  } else if (phrase === "last month") {
    start = new Date(today.getFullYear(), today.getMonth() - 1, 1);
    end = endOfMonth(start.getFullYear(), start.getMonth());
  } else if (phrase === "this year") {
    start = new Date(today.getFullYear(), 0, 1);
  } else if (phrase === "last year") {
    start = new Date(today.getFullYear() - 1, 0, 1);
    end = endOfMonth(today.getFullYear() - 1, 11);
  } else {
    const amount = Number(match[2]);
    const unit = match[3].replace(/s$/, "");
    start = new Date(today);
    if (unit === "day") start.setDate(start.getDate() - amount);
    if (unit === "week") start.setDate(start.getDate() - amount * 7);
    if (unit === "month") start.setMonth(start.getMonth() - amount);
    if (unit === "year") start.setFullYear(start.getFullYear() - amount);
  }

  return { spans: [[match.index, match.index + match[0].length]], start, end };
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
    text = blank(text, [[index, index + match[0].length]]);
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
      text = blank(text, [[index, index + match[0].length]]);
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
