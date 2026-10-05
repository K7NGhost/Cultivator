import { Badge } from "@/components/ui/badge";

type PlaceholderPageProps = {
  title: string;
  description: string;
};

export function PlaceholderPage({ title, description }: PlaceholderPageProps) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <section className="flex h-9 items-center justify-between border-b px-2">
        <div>
          <h1 className="text-sm font-semibold">{title}</h1>
          <p className="text-[11px] text-muted-foreground">{description}</p>
        </div>
        <Badge variant="outline" className="h-5 rounded-sm text-[11px]">
          Planned workspace
        </Badge>
      </section>
      <section className="flex flex-1 items-center justify-center p-2">
        <div className="w-full max-w-lg rounded-sm border bg-muted/30 p-3 text-sm">
          <div className="text-xs font-medium">{title}</div>
          <p className="mt-1 text-xs text-muted-foreground">
            This route is wired and ready for the next implementation pass.
          </p>
        </div>
      </section>
    </div>
  );
}
