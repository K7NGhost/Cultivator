export type DataSourceType = "logicalFiles";

export type DataSourcePluginType =
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
  type: DataSourcePluginType;
  mode: "each_file" | "path_regex";
  pathRegex?: string;
  entry: string;
  function: string;
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
