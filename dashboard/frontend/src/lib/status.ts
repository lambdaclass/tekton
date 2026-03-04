import { Check, X, Clock, Loader2, type LucideIcon } from "lucide-react";

export function statusVariant(status: string): {
  variant: "default" | "secondary" | "destructive" | "outline";
  className?: string;
  icon?: LucideIcon;
  spin?: boolean;
} {
  switch (status) {
    case "completed":
      return {
        variant: "outline",
        className: "text-emerald-400/80 border-emerald-400/20",
        icon: Check,
      };
    case "failed":
      return {
        variant: "outline",
        className: "text-red-400/80 border-red-400/20",
        icon: X,
      };
    case "pending":
      return {
        variant: "outline",
        className: "text-muted-foreground",
        icon: Clock,
      };
    case "awaiting_followup":
      return {
        variant: "outline",
        className: "text-amber-400/80 border-amber-400/20",
        icon: Clock,
      };
    case "creating_agent":
    case "cloning":
    case "running_claude":
    case "pushing":
    case "creating_preview":
      return {
        variant: "outline",
        className: "text-blue-400/70 border-blue-400/20",
        icon: Loader2,
        spin: true,
      };
    default:
      return { variant: "outline" };
  }
}
