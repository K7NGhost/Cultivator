export type CaseRecord = {
  id: string;
  name: string;
  examiner: string;
  reference: string;
  description: string;
  folderPath: string;
  databasePath: string;
  createdAt: string;
  updatedAt: string;
};

export type CreateCaseInput = {
  name: string;
  examiner: string;
  reference: string;
  description: string;
  parentDirectory: string;
};
