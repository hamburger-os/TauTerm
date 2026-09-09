export type BitOp =
  | "AND"
  | "OR"
  | "XOR"
  | "NOT"
  | "LSHIFT"
  | "RSHIFT"
  | "URSHIFT";

export type BitWidth = 8 | 16 | 32 | 64;
export type StructAbi = "ILP32" | "LP64" | "LLP64";
export type StructPack = 0 | 1 | 2 | 4 | 8;

export interface BitOpResult {
  unsigned: string;
  signed: string;
  bits: string;
  hex: string;
  width: BitWidth;
}

export interface BitRangeResult {
  maskHex: string;
  valueHex: string;
  unsigned: string;
}

export interface MemberInfo {
  name: string;
  type: string;
  offset: number;
  size: number;
  paddingBefore: number;
}

export interface StructInfo {
  members: MemberInfo[];
  totalSize: number;
  alignment: number;
  tailPadding: number;
  abi: StructAbi;
  pack: StructPack;
}

export const OP_KEYS: BitOp[] = [
  "AND",
  "OR",
  "XOR",
  "NOT",
  "LSHIFT",
  "RSHIFT",
  "URSHIFT",
];

function widthMask(width: BitWidth): bigint {
  return (1n << BigInt(width)) - 1n;
}

export function parseIntegerInput(
  input: string,
  width: BitWidth = 32,
): bigint | null {
  const source = input.trim().replace(/_/g, "");
  if (!source) return null;
  const match = source.match(
    /^([+-]?)(?:(0[xX])([0-9a-fA-F]+)|(0[bB])([01]+)|(\d+))$/,
  );
  if (!match) return null;

  const negative = match[1] === "-";
  const digits = match[3] ?? match[5] ?? match[6];
  const prefix = match[2] ? "0x" : match[4] ? "0b" : "";
  try {
    let value = BigInt(prefix + digits);
    if (negative) value = -value;

    const minSigned = -(1n << BigInt(width - 1));
    const maxUnsigned = widthMask(width);
    if (value < minSigned || value > maxUnsigned) return null;
    return value;
  } catch {
    return null;
  }
}

function groupedBits(value: bigint, width: BitWidth): string {
  const source = BigInt.asUintN(width, value)
    .toString(2)
    .padStart(width, "0");
  return source.replace(/(.{4})(?=.)/g, "$1 ");
}

function formatHex(value: bigint, width: BitWidth): string {
  return BigInt.asUintN(width, value)
    .toString(16)
    .toUpperCase()
    .padStart(width / 4, "0");
}

export function bitwiseOp(
  a: bigint,
  b: bigint,
  op: BitOp,
  width: BitWidth = 32,
): BitOpResult {
  const shift = Number(b);
  const aUnsigned = BigInt.asUintN(width, a);
  const bUnsigned = BigInt.asUintN(width, b);
  let result: bigint;

  switch (op) {
    case "AND": result = aUnsigned & bUnsigned; break;
    case "OR": result = aUnsigned | bUnsigned; break;
    case "XOR": result = aUnsigned ^ bUnsigned; break;
    case "NOT": result = ~aUnsigned; break;
    case "LSHIFT": result = aUnsigned << BigInt(shift); break;
    case "RSHIFT":
      result = BigInt.asIntN(width, aUnsigned) >> BigInt(shift);
      break;
    case "URSHIFT": result = aUnsigned >> BigInt(shift); break;
    default: result = 0n;
  }

  const normalized = BigInt.asUintN(width, result);
  return {
    unsigned: normalized.toString(10),
    signed: BigInt.asIntN(width, normalized).toString(10),
    bits: groupedBits(normalized, width),
    hex: formatHex(normalized, width),
    width,
  };
}

export function toggleBit(
  value: bigint,
  bit: number,
  width: BitWidth,
): bigint {
  if (bit < 0 || bit >= width) return BigInt.asUintN(width, value);
  return BigInt.asUintN(width, value ^ (1n << BigInt(bit)));
}

export function extractBitRange(
  value: bigint,
  high: number,
  low: number,
  width: BitWidth,
): BitRangeResult | null {
  if (low < 0 || high < low || high >= width) return null;
  const count = high - low + 1;
  const mask = ((1n << BigInt(count)) - 1n) << BigInt(low);
  const normalized = BigInt.asUintN(width, value);
  const extracted = (normalized & mask) >> BigInt(low);
  return {
    maskHex: formatHex(mask, width),
    valueHex: extracted.toString(16).toUpperCase().padStart(Math.ceil(count / 4), "0"),
    unsigned: extracted.toString(10),
  };
}

interface TypeInfo {
  size: number;
  align: number;
}

function abiTypeSizes(abi: StructAbi): Record<string, TypeInfo> {
  const pointer = abi === "ILP32" ? 4 : 8;
  const long = abi === "LP64" ? 8 : 4;
  const longAlign = long;
  return {
    char: { size: 1, align: 1 },
    "signed char": { size: 1, align: 1 },
    "unsigned char": { size: 1, align: 1 },
    int8_t: { size: 1, align: 1 },
    uint8_t: { size: 1, align: 1 },
    short: { size: 2, align: 2 },
    "signed short": { size: 2, align: 2 },
    "unsigned short": { size: 2, align: 2 },
    int16_t: { size: 2, align: 2 },
    uint16_t: { size: 2, align: 2 },
    int: { size: 4, align: 4 },
    "signed int": { size: 4, align: 4 },
    "unsigned int": { size: 4, align: 4 },
    int32_t: { size: 4, align: 4 },
    uint32_t: { size: 4, align: 4 },
    float: { size: 4, align: 4 },
    long: { size: long, align: longAlign },
    "signed long": { size: long, align: longAlign },
    "unsigned long": { size: long, align: longAlign },
    "long long": { size: 8, align: 8 },
    "signed long long": { size: 8, align: 8 },
    "unsigned long long": { size: 8, align: 8 },
    int64_t: { size: 8, align: 8 },
    uint64_t: { size: 8, align: 8 },
    double: { size: 8, align: 8 },
    pointer: { size: pointer, align: pointer },
  };
}

function normalizeTypeName(raw: string): { lookup: string; display: string } {
  const compact = raw.replace(/\s+/g, " ").trim();
  if (compact.endsWith("*")) {
    return { lookup: "pointer", display: compact };
  }
  return { lookup: compact, display: compact };
}

function effectiveAlign(natural: number, pack: StructPack): number {
  return pack === 0 ? natural : Math.min(natural, pack);
}

export function parseStructDefinition(
  code: string,
  abi: StructAbi = "ILP32",
  pack: StructPack = 0,
): StructInfo | null {
  let body = code.trim();
  const braceMatch = body.match(/struct\s*(?:\w+)?\s*\{([^}]*)\}/s);
  if (braceMatch) body = braceMatch[1].trim();
  if (!body.includes(";") && !braceMatch) return null;

  const declarations = body
    .split(";")
    .map((value) => value.trim())
    .filter(Boolean);
  if (declarations.length === 0) return null;

  const typeSizes = abiTypeSizes(abi);
  const members: MemberInfo[] = [];
  let currentOffset = 0;
  let maxAlignment = 1;

  for (const declaration of declarations) {
    if (declaration.includes(":") || /\b(union|struct)\b/.test(declaration)) {
      return null;
    }

    const arrayMatch = declaration.match(/^(.+?)\s+(\w+)\s*\[(\d+)\]\s*$/);
    const simpleMatch = declaration.match(/^(.+?)\s+(\w+)\s*$/);

    let rawType: string;
    let memberName: string;
    let count = 1;

    if (arrayMatch) {
      rawType = arrayMatch[1].trim();
      memberName = arrayMatch[2];
      count = Number.parseInt(arrayMatch[3], 10);
      if (!Number.isSafeInteger(count) || count <= 0) return null;
    } else if (simpleMatch) {
      rawType = simpleMatch[1].trim();
      memberName = simpleMatch[2];
    } else {
      return null;
    }

    const normalizedType = normalizeTypeName(rawType);
    const typeInfo = typeSizes[normalizedType.lookup];
    if (!typeInfo) return null;

    const alignment = effectiveAlign(typeInfo.align, pack);
    const paddingBefore =
      (alignment - (currentOffset % alignment)) % alignment;
    const offset = currentOffset + paddingBefore;
    const size = typeInfo.size * count;

    members.push({
      name: memberName,
      type: normalizedType.display + (count > 1 ? "[" + count + "]" : ""),
      offset,
      size,
      paddingBefore,
    });

    currentOffset = offset + size;
    maxAlignment = Math.max(maxAlignment, alignment);
  }

  const structAlignment = effectiveAlign(maxAlignment, pack);
  const tailPadding =
    (structAlignment - (currentOffset % structAlignment)) % structAlignment;

  return {
    members,
    totalSize: currentOffset + tailPadding,
    alignment: structAlignment,
    tailPadding,
    abi,
    pack,
  };
}

export function getSupportedTypes(abi: StructAbi = "ILP32"): string[] {
  return Object.keys(abiTypeSizes(abi))
    .filter((type) => type !== "pointer")
    .sort();
}
