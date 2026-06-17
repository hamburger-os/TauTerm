import type { TransferConfig } from "../../../types/transfer";
import YmodemConfigForm from "./forms/YmodemConfigForm";
import XmodemConfigForm from "./forms/XmodemConfigForm";
import ZmodemConfigForm from "./forms/ZmodemConfigForm";

interface ProtocolConfigFormProps {
  config: TransferConfig;
  onChange: (config: TransferConfig) => void;
}

/**
 * 按协议分发的配置表单
 * 根据 config.protocol 渲染对应的配置表单组件
 */
export default function ProtocolConfigForm({
  config,
  onChange,
}: ProtocolConfigFormProps) {
  switch (config.protocol) {
    case "ymodem":
      return <YmodemConfigForm config={config} onChange={onChange} />;
    case "xmodem":
      return <XmodemConfigForm config={config} onChange={onChange} />;
    case "zmodem":
      return <ZmodemConfigForm config={config} onChange={onChange} />;
    default:
      return null;
  }
}
