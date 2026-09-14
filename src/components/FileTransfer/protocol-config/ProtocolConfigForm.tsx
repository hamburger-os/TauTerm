import type { TransferConfig } from "../../../types/transfer";
import XmodemConfigForm from "./forms/XmodemConfigForm";
import YmodemConfigForm from "./forms/YmodemConfigForm";
import ZmodemConfigForm from "./forms/ZmodemConfigForm";

interface ProtocolConfigFormProps {
  config: TransferConfig;
  onChange: (config: TransferConfig) => void;
}

/** 只渲染本机角色真正能决定、且后端状态机会消费的协议参数。 */
export default function ProtocolConfigForm({ config, onChange }: ProtocolConfigFormProps) {
  switch (config.protocol) {
    case "ymodem":
      return <YmodemConfigForm config={config} onChange={onChange} />;
    case "xmodem":
      return <XmodemConfigForm config={config} onChange={onChange} />;
    case "zmodem":
      return <ZmodemConfigForm config={config} onChange={onChange} />;
    case "sftp":
      return null;
  }
}
