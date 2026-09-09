export interface ToolError {
  code: string;
  detail?: string;
  position?: number;
}

export type ToolResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: ToolError };

export function toolOk<T>(value: T): ToolResult<T> {
  return { ok: true, value };
}

export function toolErr(
  code: string,
  detail?: string,
  position?: number,
): ToolResult<never> {
  return {
    ok: false,
    error: {
      code,
      ...(detail ? { detail } : {}),
      ...(position !== undefined ? { position } : {}),
    },
  };
}
