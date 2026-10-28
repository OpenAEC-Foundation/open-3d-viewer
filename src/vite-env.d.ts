/// <reference types="vite/client" />

/** App version from package.json (vite.config.ts). */
declare const __APP_VERSION__: string;
/** Build tag: yyyymmdd.hhmm.<git commit>[+] (vite.config.ts). */
declare const __APP_BUILD__: string;

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

  function assimpjs(options?: { locateFile?: (path: string) => string }): Promise<AssimpJS>;
  export default assimpjs;
}

declare module 'occt-import-js' {
  function occtimportjs(options?: { locateFile?: (path: string) => string }): Promise<any>;
  export default occtimportjs;
}
