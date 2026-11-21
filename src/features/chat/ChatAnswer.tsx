import { Fragment } from "react";
import type { EvidenceSource } from "./chatApi";

export function ChatAnswer({ text, sources, select }: { text: string; sources: EvidenceSource[]; select: (source: EvidenceSource) => void }) {
  return <div className="whitespace-pre-wrap break-words text-sm leading-7 select-text">{text.split(/(\[E\d+\])/g).map((part, index) => {
    const source = sources.find(source => `[${source.id}]` === part);
    return source ? <button key={index} className="mx-0.5 inline-flex rounded-sm border bg-primary/10 px-1 text-xs font-medium text-primary hover:bg-primary/20" title={source.source} onClick={() => select(source)}>{source.id}</button> : <Fragment key={index}>{part}</Fragment>;
  })}</div>;
}
