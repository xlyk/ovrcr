import { initializeOvrcr } from "../src/ovrcr-pi-runtime.ts";

// Managed reporting injection selects the runtime owner for this invocation.
export default async function (pi) {
  await initializeOvrcr(pi, { entry: "package" });
}
