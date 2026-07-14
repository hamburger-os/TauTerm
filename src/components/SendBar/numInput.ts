/**
 * 将数字输入框的原始字符串钳制为 [min, max] 内的数字，非法输入（NaN）回退到 min。
 *
 * 用于受控 `<input type="number">` 的 onChange：清空输入框或输入非法字符时
 * `Number("")`/`Number("abc")` 会得到 `0`/`NaN`，`Math.max(min, NaN) = NaN`
 * 会被写入 state 与 localStorage。此函数统一兜底，避免 NaN 污染。
 */
export function clampNumber(raw: string, min: number, max?: number): number {
  const n = Number(raw);
  if (Number.isNaN(n)) return min;
  const clamped = Math.max(min, n);
  return max !== undefined ? Math.min(max, clamped) : clamped;
}
