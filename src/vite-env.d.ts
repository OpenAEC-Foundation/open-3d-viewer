/// <reference types="vite/client" />

declare module 'assimpjs' {
  interface AssimpFile {
    GetContent(): Uint8Array;
  }

  interface AssimpResult {
    IsSuccess(): boolean;
    FileCount(): number;
    GetFile(index: number): AssimpFile;
    GetErrorCode(): string;
  }

  interface AssimpFileList {
    AddFile(name: string, content: Uint8Array): void;
  }

  interface AssimpJS {
    FileList: new () => AssimpFileList;
    ConvertFileList(fileList: AssimpFileList, format: string): AssimpResult;
  }

  function assimpjs(): Promise<AssimpJS>;
  export default assimpjs;
}
