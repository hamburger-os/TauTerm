import { invoke } from "@tauri-apps/api/core";

export const REMOTE_DOCUMENT_PREVIEW_LIMIT = 1_048_576;
export const REMOTE_DOCUMENT_EDIT_LIMIT = 4 * 1_048_576;
export const REMOTE_DOCUMENT_HEX_LIMIT = 128 * 1024;

export type RemoteDocumentEncoding =
  | "utf-8"
  | "utf-16le"
  | "utf-16be"
  | "gb18030"
  | "big5"
  | "shift_jis"
  | "euc-jp"
  | "euc-kr"
  | "windows-1252";

export type RemoteLineEnding = "lf" | "crlf" | "cr";

export interface RemoteDocumentFormat {
  encoding: RemoteDocumentEncoding;
  lineEnding: RemoteLineEnding;
  bom: boolean;
}

export interface RemoteDocumentVersion {
  size: number;
  modified: number | null;
  contentCrc32: number | null;
}

export interface RemoteDocumentReadResult {
  data: number[];
  totalSize: number;
  modified: number | null;
  permissions: string;
  truncated: boolean;
  editable: boolean;
  version: RemoteDocumentVersion;
}

export interface RemoteDocumentSaveResult {
  status: "saved" | "conflict";
  version: RemoteDocumentVersion | null;
  currentVersion: RemoteDocumentVersion | null;
}

export interface RemoteDocumentEncodeResult {
  data: number[];
  totalSize: number;
  truncated: boolean;
}

export function openRemoteDocument(
  sessionId: string,
  remotePath: string,
  full: boolean,
): Promise<RemoteDocumentReadResult> {
  return invoke<RemoteDocumentReadResult>("sftp_read_document_cmd", {
    sessionId,
    remotePath,
    full,
  });
}

export function saveRemoteDocument(
  sessionId: string,
  remotePath: string,
  text: string,
  format: RemoteDocumentFormat,
  expectedVersion: RemoteDocumentVersion,
  force = false,
): Promise<RemoteDocumentSaveResult> {
  return invoke<RemoteDocumentSaveResult>("sftp_save_document_cmd", {
    request: {
      sessionId,
      remotePath,
      text,
      format,
      expectedVersion,
      force,
    },
  });
}

export function encodeRemoteDocumentPreview(
  text: string,
  format: RemoteDocumentFormat,
): Promise<RemoteDocumentEncodeResult> {
  return invoke<RemoteDocumentEncodeResult>("remote_document_encode_preview_cmd", {
    request: { text, format },
  });
}
