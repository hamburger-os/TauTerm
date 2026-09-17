import { useTranslation } from "react-i18next";
import ConfirmDialog from "../common/ConfirmDialog";

interface DeleteConfirmationDialogProps {
  message: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function DeleteConfirmationDialog({
  message,
  onConfirm,
  onCancel,
}: DeleteConfirmationDialogProps) {
  const { t } = useTranslation();

  return (
    <ConfirmDialog
      open={message !== null}
      title={t("fileManager.deleteConfirmTitle")}
      message={message}
      intent="danger"
      size="compact"
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
