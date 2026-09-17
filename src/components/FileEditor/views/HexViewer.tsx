import { useMemo } from "react";
import { formatRemoteDocumentHex } from "../documentCodec";
import styles from "./HexViewer.module.css";

interface HexViewerProps {
  data: Uint8Array;
}

export default function HexViewer({ data }: HexViewerProps) {
  const text = useMemo(() => formatRemoteDocumentHex(data), [data]);
  return <pre className={styles.hex}>{text}</pre>;
}
