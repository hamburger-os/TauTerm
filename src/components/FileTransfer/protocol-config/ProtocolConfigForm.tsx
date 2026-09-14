import type { TransferConfig } from "../../../types/transfer";
import YmodemConfigForm from "./forms/YmodemConfigForm";

interface ProtocolConfigFormProps {
  config: TransferConfig;
  onChange: (config: TransferConfig) => void;
}

/**
 * 只渲染真实会影响后端协议执行的可配置项。
 * XModem/ZModem 的变体与能力由握手自动协商，不显示不会实际生效的伪配置。
 */
export default function ProtocolConfigForm({
  config,
  onChange,
}: ProtocolConfigFormProps) {
  if (config.protocol === "ymodem") {
    return <YmodemConfigForm config={config} onChange={onChange} />;
  }
  return null;
}
