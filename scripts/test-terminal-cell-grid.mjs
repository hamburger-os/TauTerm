/**
 * Terminal cell-grid contract.
 *
 * xterm's DOM renderer derives the CSS row height from an integer device-pixel
 * cell divided by the display scale, so fractional display scales can produce
 * fractional rows and split box-drawing columns into dashes. This test pins the
 * planner that removes that phase for every supported scale/font combination.
 */
import assert from "node:assert/strict";
import {
  devicePixelQuantum,
  planIntegerCellHeight,
} from "../src/core/terminal-cell-grid.ts";

assert.equal(devicePixelQuantum(1), 1);
assert.equal(devicePixelQuantum(2), 1);
assert.equal(devicePixelQuantum(1.25), 4);
assert.equal(devicePixelQuantum(1.5), 2);
assert.equal(devicePixelQuantum(1.75), 4);
assert.equal(devicePixelQuantum(3), 1);
assert.equal(devicePixelQuantum(0), null);
assert.equal(devicePixelQuantum(Number.NaN), null);

const scales = [1, 1.25, 1.5, 1.75, 2, 2.5, 3];
const fontBoxHeights = [14.6, 15.0, 16.27, 16.4, 16.82, 17.55];
const lineHeights = [1, 1.02, 1.0455, 1.1];

let planned = 0;
let alreadyAligned = 0;
for (const dpr of scales) {
  for (const fontBoxHeight of fontBoxHeights) {
    for (const lineHeight of lineHeights) {
      const plan = planIntegerCellHeight({ lineHeight, fontBoxHeight, devicePixelRatio: dpr });
      if (!plan) {
        alreadyAligned += 1;
        continue;
      }
      planned += 1;
      const deviceCharHeight = Math.ceil(fontBoxHeight * dpr);
      // The chosen line height must produce exactly the planned device cell and a
      // whole CSS row height for every row count.
      assert.equal(
        Math.floor(deviceCharHeight * plan.lineHeight),
        plan.deviceCellHeight,
        `device cell mismatch for dpr=${dpr} font=${fontBoxHeight} lineHeight=${plan.lineHeight}`,
      );
      assert.ok(plan.lineHeight >= 0.9 && plan.lineHeight <= 1.35, "line height must stay sane");
      for (const rows of [1, 7, 24, 30, 40, 41, 67, 120]) {
        const cssCanvasHeight = Math.round((plan.deviceCellHeight * rows) / dpr);
        assert.ok(
          Number.isInteger(cssCanvasHeight / rows),
          `fractional CSS row for dpr=${dpr} rows=${rows}: ${cssCanvasHeight / rows}`,
        );
      }
    }
  }
}

// 200% scale with an odd device cell is the reported failure mode: it must be
// planned into whole CSS rows instead of leaving 16.5px rows behind.
const reported = planIntegerCellHeight({ lineHeight: 1, fontBoxHeight: 16.27, devicePixelRatio: 2 });
assert.ok(reported, "200% scale with an odd device cell must be aligned");
assert.equal(reported.cssCellHeight, 17);
assert.ok(planned > 0, "the planner must actually produce plans");
assert.ok(alreadyAligned > 0, "already aligned scales must stay untouched");

console.log(`terminal-cell-grid: ${planned} fractional configurations aligned, ${alreadyAligned} already integral`);
