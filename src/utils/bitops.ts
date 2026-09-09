// ── 位运算与 C 结构体 sizeof 工具函数 ──────────────────────────

// ══════════════════════════════════════════════════════════════════
// 类型定义
// ══════════════════════════════════════════════════════════════════

export type BitOp = "AND" | "OR" | "XOR" | "NOT" | "LSHIFT" | "RSHIFT";

export interface BitOpResult {
  result: number;
  /** 结果的 32 位二进制表示字符串（空格每 4 位分组） */
  bits: string;
  hex: string;
}

export interface MemberInfo {
  name: string;
  type: string;
  offset: number;
  size: number;
  /** 该成员之后的填充字节数 */
  padding: number;
}

export interface StructInfo {
  members: MemberInfo[];
  totalSize: number;
  /** 结构体对齐要求 */
  alignment: number;
}

// ══════════════════════════════════════════════════════════════════
// 位运算
// ══════════════════════════════════════════════════════════════════

/** 将数字格式化为指定位宽的二进制字符串（每 4 位空格分组） */
export function formatBits(value: number, width: number): string {
  const str = (value >>> 0).toString(2).padStart(width, "0");
  // 每 4 位插入空格
  return str.replace(/(.{4})(?=.)/g, "$1 ");
}

/** 将数字格式化为 HEX 字符串 */
export function formatHex(value: number, width: number): string {
  const hexLen = Math.ceil(width / 4);
  return (value >>> 0).toString(16).toUpperCase().padStart(hexLen, "0");
}

export const OP_KEYS: BitOp[] = ["AND", "OR", "XOR", "NOT", "LSHIFT", "RSHIFT"];

/**
 * 严格解析 32 位位运算输入。
 * 支持十进制、0x HEX、0b 二进制；不接受带非法尾缀的部分数字。
 */
export function parseIntegerInput(input: string): number | null {
  const text = input.trim().replace(/_/g, "");
  if (!text) return null;

  const match = text.match(/^([+-]?)(?:(0[xX])([0-9a-fA-F]+)|(0[bB])([01]+)|(\d+))$/);
  if (!match) return null;

  const negative = match[1] === "-";
  const digits = match[3] ?? match[5] ?? match[6];
  const radix = match[2] ? 16 : match[4] ? 2 : 10;

  let value = BigInt(radix === 10 ? digits : `${radix === 16 ? "0x" : "0b"}${digits}`);
  if (negative) value = -value;

  if (value < -0x80000000n || value > 0xFFFFFFFFn) return null;
  return Number(value);
}

/** 执行位运算 */
export function bitwiseOp(a: number, b: number, op: BitOp): BitOpResult {
  let result: number;
  switch (op) {
    case "AND": result = a & b; break;
    case "OR":  result = a | b; break;
    case "XOR": result = a ^ b; break;
    case "NOT": result = ~a; break;
    case "LSHIFT": result = a << b; break;
    case "RSHIFT": result = a >> b; break;
    default: result = 0;
  }
  return {
    result: result >>> 0,
    bits: formatBits(result, 32),
    hex: formatHex(result, 32),
  };
}

// ══════════════════════════════════════════════════════════════════
// C 结构体 sizeof 解析
// ══════════════════════════════════════════════════════════════════

// 常见 C 类型大小和对齐（ILP32 / 嵌入式 32 位通用值）
// 注意：64 位系统中，LP64 的 long/pointer 为 8 字节；
// LLP64 的 long 为 4 字节、pointer 为 8 字节。
// 当前表格以嵌入式 32 位场景为目标，非 64 位 ABI。
const TYPE_SIZES: Record<string, { size: number; align: number }> = {
  char: { size: 1, align: 1 },
  "signed char": { size: 1, align: 1 },
  "unsigned char": { size: 1, align: 1 },
  "int8_t": { size: 1, align: 1 },
  "uint8_t": { size: 1, align: 1 },
  short: { size: 2, align: 2 },
  "signed short": { size: 2, align: 2 },
  "unsigned short": { size: 2, align: 2 },
  "int16_t": { size: 2, align: 2 },
  "uint16_t": { size: 2, align: 2 },
  int: { size: 4, align: 4 },
  "signed int": { size: 4, align: 4 },
  "unsigned int": { size: 4, align: 4 },
  "int32_t": { size: 4, align: 4 },
  "uint32_t": { size: 4, align: 4 },
  float: { size: 4, align: 4 },
  long: { size: 4, align: 4 },
  "unsigned long": { size: 4, align: 4 },
  "long long": { size: 8, align: 8 },
  "unsigned long long": { size: 8, align: 8 },
  "int64_t": { size: 8, align: 8 },
  "uint64_t": { size: 8, align: 8 },
  double: { size: 8, align: 8 },
  pointer: { size: 4, align: 4 },
  "void*": { size: 4, align: 4 },
  "char*": { size: 4, align: 4 },
  "int*": { size: 4, align: 4 },
};

/**
 * 解析简单的 C 结构体定义。
 * 支持格式：
 *   struct { char a; int b; char c; }
 *   struct MyStruct { uint8_t a; uint16_t b; }
 *   或直接写成成员列表：
 *   char a; int b; char c;
 *
 * 支持数组声明：char name[16], int arr[4]
 * 不处理嵌套结构体、位域、联合体。
 */
export function parseStructDefinition(code: string): StructInfo | null {
  // 去掉 struct 关键字和类型名，提取花括号内内容
  let body = code.trim();

  // 处理 "struct { ... }" 或 "struct Name { ... }"
  const braceMatch = body.match(/struct\s*(?:\w+)?\s*\{([^}]*)\}/s);
  if (braceMatch) {
    body = braceMatch[1].trim();
  }

  // 也支持无花括号的直接成员列表
  if (!body.includes(";") && !braceMatch) {
    return null;
  }

  // 按分号分割成员
  const memberDecls = body
    .split(";")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);

  if (memberDecls.length === 0) return null;

  const members: MemberInfo[] = [];
  let currentOffset = 0;
  let maxAlign = 1;

  for (const decl of memberDecls) {
    // 解析 "type name" 或 "type name[count]"
    // 支持 "unsigned int x", "unsigned long long x" 等多词类型
    const arrayMatch = decl.match(/^(.+?)\s+(\w+)\s*\[(\d+)\]\s*$/);
    const simpleMatch = decl.match(/^(.+?)\s+(\w+)\s*$/);

    let typeName: string;
    let memberName: string;
    let arrayCount = 1;

    if (arrayMatch) {
      typeName = arrayMatch[1].trim();
      memberName = arrayMatch[2];
      arrayCount = parseInt(arrayMatch[3], 10);
    } else if (simpleMatch) {
      typeName = simpleMatch[1].trim();
      memberName = simpleMatch[2];
    } else {
      return null; // 声明无法完整解析时失败，避免静默忽略成员后给出错误 sizeof
    }

    const typeInfo = TYPE_SIZES[typeName];
    if (!typeInfo) return null; // 未知类型同样显式失败

    const memberSize = typeInfo.size * arrayCount;
    const align = typeInfo.align;

    // 当前偏移对齐
    const padding = (align - (currentOffset % align)) % align;
    const memberOffset = currentOffset + padding;

    members.push({
      name: memberName,
      type: typeName + (arrayCount > 1 ? `[${arrayCount}]` : ""),
      offset: memberOffset,
      size: memberSize,
      padding,
    });

    currentOffset = memberOffset + memberSize;
    maxAlign = Math.max(maxAlign, align);
  }

  if (members.length === 0) return null;

  // 结构体末尾对齐填充
  const tailPadding = (maxAlign - (currentOffset % maxAlign)) % maxAlign;
  const totalSize = currentOffset + tailPadding;

  return {
    members,
    totalSize,
    alignment: maxAlign,
  };
}

/**
 * 获取所有支持的类型名称
 */
export function getSupportedTypes(): string[] {
  return Object.keys(TYPE_SIZES).sort();
}
