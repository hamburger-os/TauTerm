import type { ModbusRequest, ValueFormat } from "./model";

export type ModbusServerArea = "coil" | "discrete_input" | "holding_register" | "input_register";

type RegisterReadRequest = Extract<ModbusRequest, { kind: "read_registers" }>;

interface ModbusWorkbenchDefaults {
  readFunctionCode: number;
  serverArea: ModbusServerArea;
  watchPeriodMs: number;
  watchRequest: RegisterReadRequest;
  registerFormat: ValueFormat;
}

// Option order follows protocol/domain order. These values describe the recommended
// initial workbench workflow and intentionally do not have to be the first option.
export const MODBUS_WORKBENCH_DEFAULTS = {
  readFunctionCode: 0x03,
  serverArea: "holding_register",
  watchPeriodMs: 1000,
  watchRequest: {
    kind: "read_registers",
    area: "holding_registers",
    address: 0,
    quantity: 1,
  },
  registerFormat: {
    value_type: "uint16",
    byte_order: "big",
    word_order: "normal",
    scale: 1,
    offset: 0,
    unit: "",
    bit: null,
  },
} satisfies ModbusWorkbenchDefaults;

export function createDefaultWatchRequest(): RegisterReadRequest {
  return { ...MODBUS_WORKBENCH_DEFAULTS.watchRequest };
}

export function createDefaultRegisterFormat(): ValueFormat {
  return { ...MODBUS_WORKBENCH_DEFAULTS.registerFormat };
}
