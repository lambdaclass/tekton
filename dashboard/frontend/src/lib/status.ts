export function statusVariant(status: string): {
  variant: "default" | "secondary" | "destructive" | "outline";
  className?: string;
} {
  switch (status) {
    case "completed":
      return {
        variant: "default",
        className: "bg-green-600 text-white border-transparent",
      };
    case "failed":
      return { variant: "destructive" };
    case "pending":
      return { variant: "outline" };
    case "awaiting_followup":
      return {
        variant: "secondary",
        className: "bg-amber-500/20 text-amber-400 border-amber-500/30",
      };
    default:
      return { variant: "secondary" };
  }
}
