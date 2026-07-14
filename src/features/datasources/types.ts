export type DataSourceType = "logicalFiles";

export type DataSourcePluginTarget =
  | "android"
  | "ios"
  | "windows"
  | "macos"
  | "infotainment"
  | "other";

export type DataSourcePlugin = {
  id: string;
  name: string;
  description: string;
  type: string;
  target: DataSourcePluginTarget;
  mode: "each_file" | "path_glob" | "path_regex";
  pathGlob?: string | string[];
  pathRegex?: string;
  entry: string;
  function: string;
  options?: PluginOptionDefinition[];
};

export type PluginOptionDefinition = {
  id: string;
  label: string;
  description?: string;
  type: "select";
  defaultValue: string;
  choices: Array<{ value: string; label: string }>;
};

export type DataSourceRecord = {
  id: string;
  caseId: string;
  name: string;
  type: DataSourceType;
  path: string;
  paths: string[];
  pluginIds: string[];
  createdAt: string;
};

export type CreateDataSourceInput = {
  caseDatabasePath: string;
  caseId: string;
  name: string;
  type: DataSourceType;
  paths: string[];
  plugins: DataSourcePlugin[];
};
