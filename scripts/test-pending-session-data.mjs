import assert from "node:assert/strict";
import { PendingSessionData } from "../src/core/pending-session-data.ts";

const pending = new PendingSessionData();
pending.push("elevated-child", new Uint8Array([80, 83, 62, 32]));
pending.push("elevated-child", new Uint8Array([114, 101, 97, 100, 121]));

assert.deepEqual(
  pending.drain("elevated-child").map((chunk) => Array.from(chunk)),
  [[80, 83, 62, 32], [114, 101, 97, 100, 121]],
  "startup bytes must survive until the elevated terminal becomes ready",
);
assert.deepEqual(pending.drain("elevated-child"), [], "drain must be one-shot");

const bounded = new PendingSessionData(4, 1);
bounded.push("first", new Uint8Array([1, 2, 3, 4, 5]));
assert.deepEqual(Array.from(bounded.drain("first")[0]), [1, 2, 3, 4]);
bounded.push("expired", new Uint8Array([1]));
bounded.push("latest", new Uint8Array([2]));
assert.deepEqual(bounded.drain("expired"), [], "old unknown sessions must be evicted");

console.log("pending-session-data: startup ordering preserved");
