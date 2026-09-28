/**
 * Reads the date part of a typed Assistant question ("between March 5 and
 * April 10", "before June 2025", "last 30 days") into a structured range.
 *
 * Dates without a year become a year-agnostic window, because an examiner
 * asking about "April" in a phone that spans three years means every April.
 */

import { blankSpans, type Span } from "./textSpans";

export type YearlessWindow = {
  /** 0-based months. `from > to` wraps over New Year, e.g. Nov – Feb. */
  from: number;
  to: number;
  /** Day-of-month bounds. Omitted means the whole month. */
  fromDay?: number;
  toDay?: number;
};

export type AssistantDateRange = {
  start: Date | null;
  end: Date | null;
  /** Year-agnostic window, e.g. "in April" or "before March 5" with no year given. */
  monthsOfYear?: YearlessWindow;
  label: string;
};

export type DateRangeExtraction = {
  /** The input with every consumed date word blanked out. */
  text: string;
  range: AssistantDateRange | null;
  notes: string[];
};

type DateMatch = {
  spans: Span[];
  start: Date | null;
  end: Date | null;
  monthsOfYear?: YearlessWindow;
};

type DatePoint = { month: number; day: number | null; year: number | null };

const MONTHS: Record<string, number> = {
  jan: 0, january: 0, feb: 1, february: 1, mar: 2, march: 2, apr: 3, april: 3,
  may: 4, jun: 5, june: 5, jul: 6, july: 6, aug: 7, august: 7, sep: 8, sept: 8,
  september: 8, oct: 9, october: 9, nov: 10, november: 10, dec: 11, december: 11,
};

const MONTH_NAMES = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

const MONTH_ALTERNATION = String.raw`jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?|aug(?:ust)?|sept?(?:ember)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?`;
const MONTH = String.raw`\b(${MONTH_ALTERNATION})\b\.?`;
const MONTH_WORD = new RegExp(String.raw`^(?:${MONTH_ALTERNATION})$`);
const DAY = String.raw`(\d{1,2})(?!\d)(?:st|nd|rd|th)?`;
const YEAR = String.raw`((?:19|20)\d{2})(?![\d/-])`;
/** A year that isn't part of a phone number or numeric date. */
const STANDALONE_YEAR = String.raw`(?<![\d+/-])((?:19|20)\d{2})(?![\d/-])`;
/** "and" names two ends of a range; the others also give it a direction. */
const RANGE_CONNECTOR = String.raw`\s*(-|to|through|thru|until|till|and)\s*`;
const MODIFIER = /\b(before|prior to|earlier than|until|till|up to|through|after|later than|since|starting|beginning)\s+$/;

/** Words right before a misspelled month that show it is meant as a date. */
const DATE_LEAD_WORDS = new Set([
  "in", "during", "from", "between", "since", "before", "after", "until", "till",
  "through", "thru", "of", "and", "to", "early", "late", "mid",
]);
/** Only long month names are corrected; "jun" or "may" typos are too ambiguous. */
const TYPO_CANDIDATES = MONTH_NAMES.map((name) => name.toLowerCase()).filter((name) => name.length >= 5);

const DATE_FORMAT = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
});

export function extractDateRange(
  input: string,
  now: Date,
  protectedWords: ReadonlySet<string> = new Set(),
): DateRangeExtraction {
  const notes: string[] = [];
  const text = correctMonthTypos(input, protectedWords, notes);
  const match =
    matchIsoRange(text) ??
    matchMonthRange(text, notes) ??
    matchSpecificDay(text) ??
    matchMonth(text) ??
    matchYear(text) ??
    matchRelative(text, now);

  if (!match) {
    return { text, range: null, notes };
  }

  let { start, end, monthsOfYear } = match;
  const spans = [...match.spans];
  const firstStart = Math.min(...spans.map(([spanStart]) => spanStart));
  const modifier = text.slice(0, firstStart).match(MODIFIER);

  if (modifier) {
    const word = modifier[1];
    const modifierSpan: Span = [firstStart - modifier[0].length, firstStart];

    if (monthsOfYear) {
      const adjusted = applyYearlessModifier(monthsOfYear, word);
      if (adjusted) {
        monthsOfYear = adjusted;
        spans.push(modifierSpan);
      } else {
        notes.push(`"${word}" was ignored because the date has no year to count from.`);
      }
    } else {
      spans.push(modifierSpan);
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
  }

  return {
    text: blankSpans(text, spans),
    range: {
      start,
      end,
      ...(monthsOfYear ? { monthsOfYear } : {}),
      label: monthsOfYear ? formatYearlessLabel(monthsOfYear) : formatDateRangeLabel(start, end),
    },
    notes,
  };
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

/** Whether `date` falls inside a year-agnostic window, in local time. */
export function inYearlessWindow(date: Date, window: YearlessWindow) {
  const value = monthDayKey(date.getMonth(), date.getDate());
  const low = monthDayKey(window.from, window.fromDay ?? 1);
  const high = monthDayKey(window.to, window.toDay ?? 31);

  return low <= high ? value >= low && value <= high : value >= low || value <= high;
}

function monthDayKey(month: number, day: number) {
  return month * 32 + day;
}

/** Days in a month of a leap year, so a yearless "Feb 29" stays valid. */
function daysInMonth(month: number) {
  return new Date(2024, month + 1, 0).getDate();
}

function formatYearlessLabel(window: YearlessWindow) {
  const hasDays = window.fromDay !== undefined || window.toDay !== undefined;

  if (!hasDays) {
    return window.from === window.to
      ? `${MONTH_NAMES[window.from]} (any year)`
      : `${MONTH_NAMES[window.from]} – ${MONTH_NAMES[window.to]} (any year)`;
  }

  const first = `${MONTH_NAMES[window.from].slice(0, 3)} ${window.fromDay ?? 1}`;
  const last = `${MONTH_NAMES[window.to].slice(0, 3)} ${window.toDay ?? daysInMonth(window.to)}`;
  return first === last ? `${first} (any year)` : `${first} – ${last} (any year)`;
}

/**
 * Turns "before April" (no year) into "January – March", and so on. Returns
 * null when the result would be empty ("before January") or when the window
 * already wraps over New Year, where "before" has no clear meaning.
 */
function applyYearlessModifier(window: YearlessWindow, word: string): YearlessWindow | null {
  if (window.from > window.to) {
    return null;
  }

  if (["before", "prior to", "earlier than"].includes(word)) {
    const firstDay = window.fromDay ?? 1;
    if (firstDay > 1) {
      return { from: 0, to: window.from, toDay: firstDay - 1 };
    }
    return window.from === 0 ? null : { from: 0, to: window.from - 1 };
  }

  if (["until", "till", "up to", "through"].includes(word)) {
    return withDays({ from: 0, to: window.to }, undefined, window.toDay);
  }

  if (["after", "later than"].includes(word)) {
    const lastDay = window.toDay ?? daysInMonth(window.to);
    if (lastDay < daysInMonth(window.to)) {
      return { from: window.to, fromDay: lastDay + 1, to: 11 };
    }
    return window.to === 11 ? null : { from: window.to + 1, to: 11 };
  }

  // since / starting / beginning
  return withDays({ from: window.from, to: 11 }, window.fromDay, undefined);
}

function withDays(window: YearlessWindow, fromDay?: number, toDay?: number): YearlessWindow {
  return {
    ...window,
    ...(fromDay !== undefined ? { fromDay } : {}),
    ...(toDay !== undefined ? { toDay } : {}),
  };
}

/**
 * Fixes a misspelled month ("apirl", "feburary") when the words around it
 * show it is meant as a date, and says so in `notes`. Known names are never
 * touched, so a contact called "Marc" is not read as March.
 */
function correctMonthTypos(text: string, protectedWords: ReadonlySet<string>, notes: string[]) {
  const tokens = Array.from(text.matchAll(/[a-z]+/g));
  let corrected = text;
  let shift = 0;

  tokens.forEach((token, index) => {
    const word = token[0];
    if (word.length < 4 || MONTH_WORD.test(word) || protectedWords.has(word)) {
      return;
    }

    const month = closestMonthName(word);
    if (!month || !isDateContext(text, tokens, index)) {
      return;
    }

    const at = (token.index ?? 0) + shift;
    corrected = corrected.slice(0, at) + month + corrected.slice(at + word.length);
    shift += month.length - word.length;
    notes.push(`Read "${word}" as ${month[0].toUpperCase()}${month.slice(1)}.`);
  });

  return corrected;
}

function isDateContext(text: string, tokens: RegExpMatchArray[], index: number) {
  const token = tokens[index];
  const previous = tokens[index - 1]?.[0];
  const next = tokens[index + 1]?.[0];
  const afterNext = tokens[index + 2]?.[0] ?? "";
  const rest = text.slice((token.index ?? 0) + token[0].length);

  return (
    (previous !== undefined && DATE_LEAD_WORDS.has(previous)) ||
    /^\s*,?\s*(?:(?:19|20)\d{2}|\d{1,2}(?:st|nd|rd|th)?)(?!\d)/.test(rest) ||
    (/^\s*-\s*[a-z]/.test(rest) && MONTH_WORD.test(next ?? "")) ||
    (["and", "to", "through", "thru", "until", "till"].includes(next ?? "") && MONTH_WORD.test(afterNext))
  );
}

function closestMonthName(word: string) {
  let best: string | null = null;
  let bestDistance = Infinity;
  let tied = false;

  for (const name of TYPO_CANDIDATES) {
    const allowed = name.length >= 7 ? 2 : 1;
    // Short names only accept swapped or mistyped letters, never a missing
    // or extra one, so real words like "marc" or "augusta" stay as typed.
    const lengthOk =
      name.length >= 7 ? Math.abs(name.length - word.length) <= allowed : name.length === word.length;
    if (name[0] !== word[0] || !lengthOk) {
      continue;
    }
    const distance = editDistance(word, name);
    if (distance > allowed) {
      continue;
    }
    if (distance < bestDistance) {
      best = name;
      bestDistance = distance;
      tied = false;
    } else if (distance === bestDistance) {
      tied = true;
    }
  }

  return tied ? null : best;
}

/** Optimal string alignment distance: edits plus adjacent swaps ("apirl"). */
function editDistance(left: string, right: string) {
  const rows = Array.from({ length: left.length + 1 }, (_, row) =>
    Array.from({ length: right.length + 1 }, (_, column) => (row === 0 ? column : column === 0 ? row : 0)),
  );

  for (let row = 1; row <= left.length; row += 1) {
    for (let column = 1; column <= right.length; column += 1) {
      const cost = left[row - 1] === right[column - 1] ? 0 : 1;
      rows[row][column] = Math.min(
        rows[row - 1][column] + 1,
        rows[row][column - 1] + 1,
        rows[row - 1][column - 1] + cost,
      );
      if (
        row > 1 &&
        column > 1 &&
        left[row - 1] === right[column - 2] &&
        left[row - 2] === right[column - 1]
      ) {
        rows[row][column] = Math.min(rows[row][column], rows[row - 2][column - 2] + 1);
      }
    }
  }

  return rows[left.length][right.length];
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

function validYearlessDay(month: number, day: number) {
  return day >= 1 && day <= daysInMonth(month);
}

function comparePoints(left: DatePoint, right: DatePoint) {
  return monthDayKey(left.month, left.day ?? 1) - monthDayKey(right.month, right.day ?? 1);
}

function firstMoment(point: DatePoint & { year: number }) {
  return point.day ? validDay(point.year, point.month, point.day) : new Date(point.year, point.month, 1);
}

function lastMoment(point: DatePoint & { year: number }) {
  if (!point.day) {
    return endOfMonth(point.year, point.month);
  }
  const day = validDay(point.year, point.month, point.day);
  return day ? endOfDay(day) : null;
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

/** "may" is an ordinary word too; only read it as a month near a year or date preposition. */
function isPlainWordMay(text: string, monthToken: string, index: number, hasYear: boolean) {
  return (
    monthToken === "may" &&
    !hasYear &&
    !/\b(in|during|from|of|since|before|after|until|between)\s+$/.test(text.slice(0, index))
  );
}

function matchIsoRange(text: string): DateMatch | null {
  const match = text.match(
    /(\d{4})-(\d{1,2})-(\d{1,2})\s+(?:-|to|through|thru|until|till|and)\s+(\d{4})-(\d{1,2})-(\d{1,2})/,
  );
  if (!match || match.index === undefined) {
    return null;
  }
  let start = validDay(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  let end = validDay(Number(match[4]), Number(match[5]) - 1, Number(match[6]));
  if (!start || !end) {
    return null;
  }
  if (start > end) {
    [start, end] = [end, start];
  }
  return {
    spans: [absorbLead(text, [match.index, match.index + match[0].length])],
    start,
    end: endOfDay(end),
  };
}

function matchMonthRange(text: string, notes: string[]): DateMatch | null {
  const match = text.match(
    new RegExp(
      String.raw`${MONTH}(?:\s+${DAY})?(?:,?\s+${YEAR})?${RANGE_CONNECTOR}${MONTH}(?:\s+${DAY})?(?:,?\s+${YEAR})?`,
    ),
  );
  if (!match || match.index === undefined) {
    return null;
  }

  let first: DatePoint = {
    month: monthIndex(match[1]),
    day: match[2] ? Number(match[2]) : null,
    year: match[3] ? Number(match[3]) : null,
  };
  let second: DatePoint = {
    month: monthIndex(match[5]),
    day: match[6] ? Number(match[6]) : null,
    year: match[7] ? Number(match[7]) : null,
  };
  const span = absorbLead(text, [match.index, match.index + match[0].length]);
  const spans: Span[] = [span];

  // "between April and March" names two ends, not a direction, so read it
  // in calendar order instead of wrapping around the whole year.
  if (match[4] === "and" && first.year === null && second.year === null && comparePoints(first, second) > 0) {
    [first, second] = [second, first];
  }

  if (first.year === null && second.year === null) {
    const standalone = findStandaloneYear(text, span);
    if (!standalone) {
      if (
        (first.day !== null && !validYearlessDay(first.month, first.day)) ||
        (second.day !== null && !validYearlessDay(second.month, second.day))
      ) {
        return null;
      }
      // "April to March" wraps into a window that keeps (nearly) every
      // month, which no one means as a filter; "Nov - Feb" is a real wrap.
      const wrappedMonths = (second.month - first.month + 12) % 12 + 1;
      if (comparePoints(first, second) > 0 && wrappedMonths >= 11) {
        [first, second] = [second, first];
        notes.push(
          `Read "${MONTH_NAMES[second.month]} to ${MONTH_NAMES[first.month]}" in calendar order, ${MONTH_NAMES[first.month]} – ${MONTH_NAMES[second.month]}.`,
        );
      }
      return {
        spans,
        start: null,
        end: null,
        monthsOfYear: withDays(
          { from: first.month, to: second.month },
          first.day ?? undefined,
          second.day ?? undefined,
        ),
      };
    }
    second = { ...second, year: standalone.year };
    spans.push(standalone.span);
  }

  const firstYear = first.year ?? (comparePoints(first, second) <= 0 ? second.year! : second.year! - 1);
  const secondYear = second.year ?? (comparePoints(second, first) >= 0 ? firstYear : firstYear + 1);
  const from = { ...first, year: firstYear };
  const to = { ...second, year: secondYear };

  let start = firstMoment(from);
  let end = lastMoment(to);
  if (start && end && start > end) {
    // Both ends were typed with years, just in the wrong order.
    start = firstMoment(to);
    end = lastMoment(from);
  }

  return start && end ? { spans, start, end } : null;
}

function matchSpecificDay(text: string): DateMatch | null {
  const candidates: Array<{ span: Span; year: number | null; month: number; day: number; monthToken?: string }> = [];

  const monthFirst = text.match(new RegExp(String.raw`${MONTH}\s+${DAY}(?:,?\s+${YEAR})?`));
  if (monthFirst?.index !== undefined) {
    candidates.push({
      span: [monthFirst.index, monthFirst.index + monthFirst[0].length],
      month: monthIndex(monthFirst[1]),
      day: Number(monthFirst[2]),
      year: monthFirst[3] ? Number(monthFirst[3]) : null,
      monthToken: monthFirst[1],
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
      if (standalone) {
        year = standalone.year;
        spans.push(standalone.span);
      }
    }

    if (year === null) {
      if (
        (candidate.monthToken && isPlainWordMay(text, candidate.monthToken, candidate.span[0], false)) ||
        !validYearlessDay(candidate.month, candidate.day)
      ) {
        continue;
      }
      return {
        spans,
        start: null,
        end: null,
        monthsOfYear: {
          from: candidate.month,
          to: candidate.month,
          fromDay: candidate.day,
          toDay: candidate.day,
        },
      };
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

  if (isPlainWordMay(text, match[1], match.index, Boolean(match[2]))) {
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
    return { spans, start: null, end: null, monthsOfYear: { from: month, to: month } };
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
