import { cn } from "@/lib/utils";

// One diff, coloured by its markers — never by parsing the code: a line of
// code that begins with a minus is not a deletion unless the marker says so.
export function Patch({ lines, truncated, className }: { lines: string[]; truncated?: boolean; className?: string }) {
  return (
    <pre className={cn("overflow-x-auto py-2 font-mono text-xs leading-[1.45]", className)}>
      {lines.map((line, i) => {
        const kind = line.startsWith("+") && !line.startsWith("+++") ? "add"
          : line.startsWith("-") && !line.startsWith("---") ? "del"
          : line.startsWith("@@") || line.startsWith("diff ") ? "meta" : "";
        return (
          <span
            key={i}
            className={cn(
              "block px-3 whitespace-pre",
              kind === "add" && "bg-success/12",
              kind === "del" && "bg-destructive/12",
              kind === "meta" && "text-muted-foreground",
            )}
          >
            {line || " "}
          </span>
        );
      })}
      {truncated && <span className="text-muted-foreground block px-3">… the rest of this file's edits are not kept</span>}
    </pre>
  );
}
