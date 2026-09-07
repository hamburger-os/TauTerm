import { useTranslation } from "react-i18next";
import styles from "./TrdpSessionView.module.css";
import {
  displayValue,
  type DecodedDataset,
  type FlowRow,
  type TrdpEvent,
  type XmlImport,
} from "./model";

type AnalysisTablesProps = {
  flows: FlowRow[];
  packetRows: TrdpEvent[];
  selectedPacket: TrdpEvent | null;
  packetTotal: number;
  packetPage: number;
  packetPageCount: number;
  packetPageSize: number;
  onPacketPageChange: (page: number) => void;
  onInspectPacket: (event: TrdpEvent) => void;
};

export function TrdpAnalysisTables({
  flows,
  packetRows,
  selectedPacket,
  packetTotal,
  packetPage,
  packetPageCount,
  packetPageSize,
  onPacketPageChange,
  onInspectPacket,
}: AnalysisTablesProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.analysisGrid}>
      <div className={styles.analysisPane}>
        <div className={styles.analysisPaneHeader}>
          <h3 className={styles.subheading}>{t("trdp.section.flows")}</h3>
        </div>
        <div className={styles.tableWrap}>
          <table className={`${styles.table} ${styles.flowTable}`}>
            <thead>
              <tr>
                <th>{t("trdp.table.link")}</th>
                <th>{t("trdp.table.type")}</th>
                <th>ComID</th>
                <th>{t("trdp.table.source")}</th>
                <th>{t("trdp.table.destination")}</th>
                <th>{t("trdp.table.packets")}</th>
                <th>{t("trdp.table.errors")}</th>
              </tr>
            </thead>
            <tbody>
              {flows.length === 0 ? (
                <tr>
                  <td colSpan={7} className={styles.emptyState}>
                    {t("trdp.empty.noTraffic")}
                  </td>
                </tr>
              ) : flows.map(flow => (
                <tr key={flow.key}>
                  <td>{flow.link}</td>
                  <td>{flow.msg}</td>
                  <td>{flow.comId}</td>
                  <td>{flow.src}</td>
                  <td>{flow.dst}</td>
                  <td>{flow.count}</td>
                  <td>{flow.errors}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className={styles.analysisPane}>
        <div className={styles.analysisPaneHeader}>
          <h3 className={styles.subheading}>{t("trdp.section.packets")}</h3>
          {packetPageCount > 1 && (
            <div className={styles.packetPager}>
              <button
                className={`${styles.compactButton} liquid-glass-button`}
                onClick={() => onPacketPageChange(Math.max(0, packetPage - 1))}
                disabled={packetPage === 0}
              >
                {t("trdp.pagination.newer")}
              </button>
              <span>{packetPage + 1} / {packetPageCount}</span>
              <button
                className={`${styles.compactButton} liquid-glass-button`}
                onClick={() => onPacketPageChange(Math.min(packetPageCount - 1, packetPage + 1))}
                disabled={packetPage >= packetPageCount - 1}
              >
                {t("trdp.pagination.older")}
              </button>
            </div>
          )}
        </div>
        <div className={styles.tableWrap}>
          <table className={`${styles.table} ${styles.monoTable} ${styles.packetTable}`}>
            <thead>
              <tr>
                <th>#</th>
                <th>{t("trdp.table.link")}</th>
                <th>{t("trdp.table.type")}</th>
                <th>ComID</th>
                <th>{t("trdp.table.sourceDestination")}</th>
                <th>{t("trdp.table.seq")}</th>
                <th>{t("trdp.table.length")}</th>
              </tr>
            </thead>
            <tbody>
              {packetRows.length === 0 ? (
                <tr>
                  <td colSpan={7} className={styles.emptyState}>
                    {t("trdp.empty.noPackets")}
                  </td>
                </tr>
              ) : packetRows.map((event, index) => (
                <tr
                  key={`${String(event.timestamp_us ?? 0)}-${index}`}
                  className={selectedPacket === event ? styles.selectedRow : ""}
                  onClick={() => onInspectPacket(event)}
                >
                  <td>{Math.max(1, packetTotal - packetPage * packetPageSize - index)}</td>
                  <td>{event.link ?? "—"}</td>
                  <td>{event.msg_type ?? event.kind ?? "—"}</td>
                  <td>{event.com_id ?? "—"}</td>
                  <td>{event.src_ip ?? "—"} → {event.dest_ip ?? "—"}</td>
                  <td>{event.seq_count ?? "—"}</td>
                  <td>{event.data_len ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

type PacketInspectorProps = {
  selectedPacket: TrdpEvent | null;
  decoded: DecodedDataset | null;
  xmlImport: XmlImport | null;
  onConfirmMessage: (event: TrdpEvent) => void;
  canConfirmMessage: boolean;
  mdLatencyUs: (event: TrdpEvent) => number | undefined;
  observedMdReplies: (event: TrdpEvent) => number | undefined;
};

export function TrdpPacketInspector({
  selectedPacket,
  decoded,
  xmlImport,
  onConfirmMessage,
  canConfirmMessage,
  mdLatencyUs,
  observedMdReplies,
}: PacketInspectorProps) {
  const { t } = useTranslation();
  return (
    <div className={`${styles.packetCard} liquid-glass-card`}>
      <h3>{t("trdp.section.packetInspector")}</h3>
      {!selectedPacket ? (
        <div className={styles.emptyInspector}>{t("trdp.empty.selectPacket")}</div>
      ) : (
        <>
          <div>
            {t("trdp.overview.protocol")} {selectedPacket.protocol_version ?? "—"} (
            {selectedPacket.protocol_valid === undefined
              ? t("trdp.inspector.notChecked")
              : selectedPacket.protocol_valid
                ? t("trdp.inspector.valid")
                : t("trdp.inspector.invalid")}
            ) · CRC {selectedPacket.crc_valid === undefined
              ? t("trdp.inspector.notChecked")
              : selectedPacket.crc_valid
                ? t("trdp.inspector.valid")
                : t("trdp.inspector.invalid")} · {t("trdp.inspector.result")} {selectedPacket.result_code ?? "—"}
          </div>
          <div>
            ComID {selectedPacket.com_id ?? "—"} · {selectedPacket.src_ip ?? "—"} → {selectedPacket.dest_ip ?? "—"} ·
            Seq {selectedPacket.seq_count ?? "—"} · ETB/Op {selectedPacket.etb_topo_count ?? "—"}/{selectedPacket.op_trn_topo_count ?? "—"}
          </div>
          {selectedPacket.md_session_id && (
            <div>
              MD Session UUID: <code>{selectedPacket.md_session_id}</code> · {t("trdp.inspector.requestReplyLatency")} {mdLatencyUs(selectedPacket) ?? "—"} µs ·
              {t("trdp.inspector.replies")} {selectedPacket.num_replies ?? observedMdReplies(selectedPacket) ?? "—"}/{selectedPacket.num_expected_replies ?? "—"}
              {selectedPacket.msg_type === "Mq" && canConfirmMessage && (
                <button
                  className={`${styles.compactButton} liquid-glass-button`}
                  onClick={() => onConfirmMessage(selectedPacket)}
                >
                  {t("trdp.actions.confirm")} (Mc)
                </button>
              )}
              {selectedPacket.msg_type === "Mq" && !canConfirmMessage && (
                <span className={styles.mutedInline}>{t("trdp.inspector.confirmUnavailable")}</span>
              )}
            </div>
          )}
          <div className={styles.payload}>
            {t("trdp.inspector.rawPayload")}: <code>{selectedPacket.payload_hex || "—"}</code>
          </div>
          {decoded ? (
            <>
              <h4>{decoded.dataset_name} · Dataset {decoded.dataset_id}</h4>
              <div>{decoded.consumed_bytes}/{decoded.payload_bytes} {t("trdp.inspector.bytesDecoded")}</div>
              <div className={styles.tableWrap}>
                <table className={styles.table}>
                  <thead>
                    <tr>
                      <th>{t("trdp.table.field")}</th>
                      <th>{t("trdp.table.type")}</th>
                      <th>{t("trdp.table.value")}</th>
                      <th>{t("trdp.table.unit")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(decoded.fields).map(([name, field]) => (
                      <tr key={name}>
                        <td>{name}</td>
                        <td>{field.type}</td>
                        <td>{field.error ?? displayValue(field.value)}</td>
                        <td>{field.unit ?? "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          ) : xmlImport && selectedPacket.com_id !== undefined ? (
            <div>{t("trdp.inspector.noMapping")} ComID {selectedPacket.com_id}.</div>
          ) : (
            <div>{t("trdp.inspector.importXmlToDecode")}</div>
          )}
        </>
      )}
    </div>
  );
}
