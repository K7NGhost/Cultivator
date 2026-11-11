type PluginFailure = {
  jobId: string;
  pluginName: string;
  error: string | null;
};

export function PluginRunFailureNotice({
  datasourceName,
  failures,
}: {
  datasourceName: string;
  failures: PluginFailure[];
}) {
  return (
    <div className="min-w-0 text-sm">
      <p className="font-medium">Plugin run failed on {datasourceName}</p>
      <ul className="mt-2 max-h-64 space-y-2 overflow-y-auto">
        {failures.map((failure) => (
          <li key={failure.jobId}>
            <p className="font-medium">{failure.pluginName}</p>
            <p className="mt-1 whitespace-pre-wrap break-words text-xs">
              {failure.error?.trim() || "The plugin stopped without recording an error message."}
            </p>
          </li>
        ))}
      </ul>
    </div>
  );
}
