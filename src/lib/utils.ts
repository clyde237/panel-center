import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatSize(bytes: number | null) {
  if (bytes === null) return "Non disponible";
  return new Intl.NumberFormat("fr-FR", {
    style: "unit",
    unit: bytes >= 1024 ** 3 ? "gigabyte" : "megabyte",
    maximumFractionDigits: 1,
  }).format(bytes / (bytes >= 1024 ** 3 ? 1024 ** 3 : 1024 ** 2));
}
