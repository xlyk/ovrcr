// Materialized as .ts beside the common runtime. The supervising binary replaces
// this token with its JSON-escaped absolute path for this invocation only.
import { initializeOvrcr } from "./ovrcr-pi-runtime.ts";

const OVRCR_BINARY = __OVRCR_BINARY__;
export default async function (pi) {
  await initializeOvrcr(pi, { entry: "injected", binary: OVRCR_BINARY });
}
