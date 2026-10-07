import { money, tokens } from "@/lib/format";
import type { Report } from "@/lib/types";

/** The report as something to paste: the figures a summary would type. */
export function reportMarkdown(r: Report): string {
  const out: string[] = ["# " + (r.title || r.project || r.session_id)];
  const meta = [r.project, r.branch, r.model || r.provider].filter(Boolean).join(" · ");
  if (meta) out.push(meta);
  out.push("");
  out.push("- Cost: " + (r.cost.included ? "included in the plan" : r.cost.available ? money(r.cost.total) : "not recorded"));
  out.push(`- Tokens: ${tokens(r.tokens.total || r.tokens.input + r.tokens.output)} (${tokens(r.tokens.input)} in · ${tokens(r.tokens.output)} out)`);
  if (r.context?.max)
    out.push(
      `- Context: ${Math.round(r.context.percent_to_compact)}% of the window (${tokens(r.context.used)} of ${tokens(r.context.max)})` +
        (r.context.compactions ? `, ${r.context.compactions} compactions` : ""),
    );
  if (r.duration) out.push("- Duration: " + r.duration + (r.running ? ", still running" : ""));
  if (r.activity?.error_rate != null)
    out.push(`- Tool errors: ${Math.round(r.activity.error_rate * 100)}% (${r.activity.tool_errors || 0} of ${r.activity.tool_count || 0})`);
  const loops = (r.activity.failures ?? []).filter((f: { count: number }) => f.count > 1);
  if (loops.length) {
    out.push("", "## Repeated failures");
    for (const f of loops.slice(0, 6)) {
      let d = String(f.detail || "").split("\n")[0];
      if (d.length > 120) d = d.slice(0, 117) + "…";
      out.push(`- ×${f.count} \`${f.tool}\`` + (d ? ` — \`${d}\`` : ""));
    }
  }
  if (r.files?.length) {
    out.push("", "## Files it wrote");
    for (const f of r.files) out.push("- " + f);
  }
  return out.join("\n");
}
