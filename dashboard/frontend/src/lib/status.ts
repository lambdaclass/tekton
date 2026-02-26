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
        variant: "default",
        className: "bg-green-600 text-white border-transparent",
        icon: Check,
      };
    case "failed":
      return { variant: "destructive", icon: X };
    case "pending":
      return { variant: "outline", icon: Clock };
    case "awaiting_followup":
      return {
        variant: "secondary",
        className: "bg-amber-500/20 text-amber-400 border-amber-500/30",
        icon: Clock,
      };
    case "creating_agent":
    case "cloning":
    case "running_claude":
    case "pushing":
    case "creating_preview":
      return { variant: "secondary", icon: Loader2, spin: true };
    default:
      return { variant: "secondary" };
  }
}
