import type { IndexedHit } from "../services/indexedSearchRepository";
import type { IndexedRecordDisplay } from "../utils/indexedRecordDisplay";

export function IndexedRecordPreview({ hit, display }: { hit: IndexedHit; display: IndexedRecordDisplay }) {
  return <div className="grid gap-3">
    <h3 className="font-medium">{display.title}</h3>
    {display.partial && <p className="text-muted-foreground">Record excerpt. Additional data may appear in adjacent chunks or the original indexed text.</p>}
    {display.text && <p className="whitespace-pre-wrap break-words leading-5 select-text">{display.text}</p>}
    {display.fields.length > 0 && <dl className="divide-y border-y" aria-label="Readable indexed fields">
      {display.fields.map((field, index) => <div key={index} className="grid grid-cols-[minmax(7rem,11rem)_minmax(0,1fr)] gap-x-3 py-2">
        <dt className="break-words text-muted-foreground">{field.label}</dt>
        <dd className="min-w-0 whitespace-pre-wrap break-words leading-5 select-text">{field.value}</dd>
      </div>)}
    </dl>}
    {hit.stage !== "2" && <details className="text-muted-foreground">
      <summary className="cursor-pointer text-[11px]">Original indexed text</summary>
      <pre className="mt-2 whitespace-pre-wrap break-words font-mono text-[11px] leading-5 select-text">{hit.body}</pre>
    </details>}
  </div>;
}
