import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  REMOTE_DOCUMENT_EDIT_LIMIT,
  REMOTE_DOCUMENT_HEX_LIMIT,
  encodeRemoteDocumentPreview,
  openRemoteDocument,
  saveRemoteDocument,
  type RemoteDocumentEncoding,
  type RemoteDocumentFormat,
  type RemoteDocumentReadResult,
  type RemoteDocumentSaveResult,
} from "../../services/remoteDocumentService";
import {
  decodeInitialRemoteDocument,
  decodeRemoteDocument,
  detectLineEnding,
  normalizeDocumentText,
} from "./documentCodec";

export type RemoteDocumentMode = "text" | "hex";

export interface RemoteDocumentSnapshot extends RemoteDocumentReadResult {
  bytes: Uint8Array;
}

function sameFormat(left: RemoteDocumentFormat | null, right: RemoteDocumentFormat): boolean {
  return Boolean(
    left
      && left.encoding === right.encoding
      && left.lineEnding === right.lineEnding
      && left.bom === right.bom,
  );
}

export function useRemoteDocument(
  sessionId: string,
  remotePath: string,
  isConnected: boolean,
  onSaved?: () => void,
) {
  const generationRef = useRef(0);
  const hexGenerationRef = useRef(0);
  const [snapshot, setSnapshot] = useState<RemoteDocumentSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<RemoteDocumentMode>("text");
  const [text, setText] = useState("");
  const [originalText, setOriginalText] = useState("");
  const [format, setFormat] = useState<RemoteDocumentFormat>({
    encoding: "utf-8",
    lineEnding: "lf",
    bom: false,
  });
  const [originalFormat, setOriginalFormat] = useState<RemoteDocumentFormat | null>(null);
  const [sourceEncoding, setSourceEncoding] = useState<RemoteDocumentEncoding>("utf-8");
  const [encodingConfirmed, setEncodingConfirmed] = useState(false);
  const [binaryLikely, setBinaryLikely] = useState(false);
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState<RemoteDocumentSaveResult | null>(null);
  const [sourceBytesStale, setSourceBytesStale] = useState(false);
  const [hexData, setHexData] = useState<Uint8Array>(new Uint8Array());
  const [hexTotalSize, setHexTotalSize] = useState(0);
  const [hexTruncated, setHexTruncated] = useState(false);
  const [hexLoading, setHexLoading] = useState(false);

  const dirty = useMemo(
    () => text !== originalText || !sameFormat(originalFormat, format),
    [format, originalFormat, originalText, text],
  );

  const canEdit = Boolean(
    snapshot
      && snapshot.editable
      && !snapshot.truncated
      && encodingConfirmed
      && !binaryLikely,
  );
  const canLoadFullForEdit = Boolean(
    snapshot
      && snapshot.truncated
      && snapshot.totalSize <= REMOTE_DOCUMENT_EDIT_LIMIT
      && encodingConfirmed
      && !binaryLikely,
  );

  const applyReadResult = useCallback((result: RemoteDocumentReadResult) => {
    const bytes = new Uint8Array(result.data);
    const decoded = decodeInitialRemoteDocument(bytes, result.truncated);
    setSnapshot({ ...result, bytes });
    setText(decoded.text);
    setOriginalText(decoded.text);
    setFormat(decoded.format);
    setOriginalFormat(decoded.format);
    setSourceEncoding(decoded.format.encoding);
    setEncodingConfirmed(decoded.encodingConfirmed);
    setBinaryLikely(decoded.binaryLikely);
    setMode(decoded.binaryLikely || !decoded.encodingConfirmed ? "hex" : "text");
    setConflict(null);
    setSourceBytesStale(false);
    setHexData(bytes.subarray(0, Math.min(bytes.length, REMOTE_DOCUMENT_HEX_LIMIT)));
    setHexTotalSize(result.totalSize);
    setHexTruncated(result.totalSize > REMOTE_DOCUMENT_HEX_LIMIT);
  }, []);

  const load = useCallback(async (full: boolean) => {
    const generation = ++generationRef.current;
    hexGenerationRef.current += 1;
    setLoading(true);
    setError(null);
    try {
      const result = await openRemoteDocument(sessionId, remotePath, full);
      if (generation !== generationRef.current) return false;
      applyReadResult(result);
      return true;
    } catch (loadError) {
      if (generation !== generationRef.current) return false;
      setError(String(loadError));
      return false;
    } finally {
      if (generation === generationRef.current) setLoading(false);
    }
  }, [applyReadResult, remotePath, sessionId]);

  useEffect(() => {
    setSnapshot(null);
    setText("");
    setOriginalText("");
    setOriginalFormat(null);
    setEncodingConfirmed(false);
    setConflict(null);
    setError(null);
    setSaving(false);
    setSourceBytesStale(false);
    void load(false);
    return () => {
      generationRef.current += 1;
      hexGenerationRef.current += 1;
    };
  }, [load]);

  const reopenAs = useCallback(async (encoding: RemoteDocumentEncoding) => {
    if (dirty) return false;

    let current = snapshot;
    if (!current || sourceBytesStale) {
      const full = Boolean(current && current.totalSize <= REMOTE_DOCUMENT_EDIT_LIMIT && current.editable);
      if (!await load(full)) return false;
      // load() commits state asynchronously; use a fresh backend read so this operation never
      // decodes a stale in-memory byte snapshot after a save.
      try {
        const result = await openRemoteDocument(sessionId, remotePath, full);
        const bytes = new Uint8Array(result.data);
        current = { ...result, bytes };
        setSnapshot(current);
        setSourceBytesStale(false);
      } catch (reopenError) {
        setError(String(reopenError));
        return false;
      }
    }

    const decodedRaw = decodeRemoteDocument(current.bytes, encoding, current.truncated);
    const normalized = normalizeDocumentText(decodedRaw);
    const nextFormat: RemoteDocumentFormat = {
      encoding,
      lineEnding: detectLineEnding(decodedRaw),
      bom: (
        (encoding === "utf-8"
          && current.bytes.length >= 3
          && current.bytes[0] === 0xef
          && current.bytes[1] === 0xbb
          && current.bytes[2] === 0xbf)
        || (encoding === "utf-16le"
          && current.bytes.length >= 2
          && current.bytes[0] === 0xff
          && current.bytes[1] === 0xfe)
        || (encoding === "utf-16be"
          && current.bytes.length >= 2
          && current.bytes[0] === 0xfe
          && current.bytes[1] === 0xff)
      ),
    };
    setSourceEncoding(encoding);
    setEncodingConfirmed(true);
    setText(normalized);
    setOriginalText(normalized);
    setFormat(nextFormat);
    setOriginalFormat(nextFormat);
    setBinaryLikely(false);
    setMode("text");
    setError(null);
    return true;
  }, [dirty, load, remotePath, sessionId, snapshot, sourceBytesStale]);

  const updateFormat = useCallback((next: Partial<RemoteDocumentFormat>) => {
    setFormat((previous) => {
      const merged = { ...previous, ...next };
      if (!merged.encoding.startsWith("utf-") && merged.bom) merged.bom = false;
      return merged;
    });
  }, []);

  const save = useCallback(async (force = false) => {
    if (!snapshot || !canEdit || !isConnected || saving) return false;
    if (snapshot.version.contentCrc32 === null) {
      setError("Document version is unavailable; reload the full document before saving.");
      return false;
    }

    setSaving(true);
    setError(null);
    setConflict(null);
    try {
      const result = await saveRemoteDocument(
        sessionId,
        remotePath,
        text,
        format,
        snapshot.version,
        force,
      );
      if (result.status === "conflict") {
        setConflict(result);
        return false;
      }
      if (!result.version) {
        setError("Remote document save completed without a version.");
        return false;
      }

      setSnapshot((previous) => previous ? {
        ...previous,
        totalSize: result.version!.size,
        modified: result.version!.modified,
        version: result.version!,
        truncated: false,
        editable: true,
      } : previous);
      setOriginalText(text);
      setOriginalFormat(format);
      setSourceEncoding(format.encoding);
      setEncodingConfirmed(true);
      setConflict(null);
      setSourceBytesStale(true);
      onSaved?.();
      return true;
    } catch (saveError) {
      setError(String(saveError));
      return false;
    } finally {
      setSaving(false);
    }
  }, [canEdit, format, isConnected, onSaved, remotePath, saving, sessionId, snapshot, text]);

  const reloadFromRemote = useCallback(async () => {
    if (!snapshot) return load(false);
    return load(snapshot.totalSize <= REMOTE_DOCUMENT_EDIT_LIMIT && !snapshot.truncated);
  }, [load, snapshot]);

  const loadFullForEdit = useCallback(() => load(true), [load]);

  useEffect(() => {
    if (mode !== "hex" || !snapshot) return;

    if (!dirty && !sourceBytesStale) {
      setHexData(snapshot.bytes.subarray(0, Math.min(snapshot.bytes.length, REMOTE_DOCUMENT_HEX_LIMIT)));
      setHexTotalSize(snapshot.totalSize);
      setHexTruncated(snapshot.totalSize > REMOTE_DOCUMENT_HEX_LIMIT);
      setHexLoading(false);
      return;
    }

    const generation = ++hexGenerationRef.current;
    setHexLoading(true);
    encodeRemoteDocumentPreview(text, format)
      .then((result) => {
        if (generation !== hexGenerationRef.current) return;
        setHexData(new Uint8Array(result.data));
        setHexTotalSize(result.totalSize);
        setHexTruncated(result.truncated);
      })
      .catch((hexError) => {
        if (generation === hexGenerationRef.current) setError(String(hexError));
      })
      .finally(() => {
        if (generation === hexGenerationRef.current) setHexLoading(false);
      });
  }, [dirty, format, mode, snapshot, sourceBytesStale, text]);

  return {
    snapshot,
    loading,
    error,
    setError,
    mode,
    setMode,
    text,
    setText,
    format,
    updateFormat,
    sourceEncoding,
    encodingConfirmed,
    reopenAs,
    binaryLikely,
    dirty,
    saving,
    conflict,
    canEdit,
    canLoadFullForEdit,
    loadFullForEdit,
    reloadFromRemote,
    save,
    hexData,
    hexTotalSize,
    hexTruncated,
    hexLoading,
  };
}
