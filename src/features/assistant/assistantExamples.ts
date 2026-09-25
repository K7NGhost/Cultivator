import type { AssistantIndex } from "@/features/assistant/assistantEngine";
import { splitParticipant } from "@/features/assistant/assistantQuery";

export function buildExamples(index: AssistantIndex | null) {
  const examples: string[] = [];
  const counts = new Map<string, number>();

  for (const message of index?.messages ?? []) {
    for (const value of [message.sender, ...message.recipients]) {
      const { name } = splitParticipant(value);
      if (name) {
        counts.set(name, (counts.get(name) ?? 0) + 1);
      }
    }
  }

  const [first, second] = Array.from(counts.entries())
    .sort((left, right) => right[1] - left[1])
    .map(([name]) => name.split(" ")[0]);
  const years = (index?.messages ?? []).map((message) => message.at?.getFullYear()).filter(Boolean);
  const latestYear = years.length > 0 ? Math.max(...(years as number[])) : null;

  examples.push(
    first && second
      ? `Messages about money between ${first} and ${second}${latestYear ? ` March - Jun ${latestYear}` : ""}`
      : "Show me all messages",
  );
  if (second) {
    examples.push(`Messages from ${second}`);
  }
  examples.push("Transactions", 'Messages containing "delete"', second ? `Calls with ${second}` : "Show me all calls");
  examples.push("Photos of vehicles", latestYear ? `Photos from July ${latestYear}` : "Photos from last month", "Contacts");

  return examples;
}
