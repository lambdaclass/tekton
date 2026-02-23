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
    default:
      return { variant: "secondary" };
  }
}
