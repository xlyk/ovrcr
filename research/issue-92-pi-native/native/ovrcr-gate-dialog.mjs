// Native-gate helper for OVRCR #92. Draws one real Pi extension dialog on demand,
// so ui_prompt_start / ui_prompt_end fire exactly as docs/extensions.md describes.
// It reads nothing, writes nothing, and sends nothing anywhere.
export default function (pi) {
  pi.registerCommand("gate-select", {
    description: "OVRCR native gate: open one real select dialog",
    handler: async (_args, ctx) => {
      const choice = await ctx.ui.select("OVRCR gate: pick one", ["one", "two", "three"]);
      ctx.ui.notify(`gate-select -> ${choice ?? "cancelled"}`, "info");
    },
  });
  pi.registerCommand("gate-confirm", {
    description: "OVRCR native gate: open one real confirm dialog",
    handler: async (_args, ctx) => {
      const ok = await ctx.ui.confirm("OVRCR gate", "Confirm?");
      ctx.ui.notify(`gate-confirm -> ${ok}`, "info");
    },
  });
}
