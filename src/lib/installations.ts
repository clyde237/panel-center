import type { ApplicationInstallation, PackageFormat } from "./contracts";

export type ApplicationsViewMode = "list" | "grid";

export const DEFAULT_APPLICATIONS_VIEW_MODE: ApplicationsViewMode = "list";

export function resolveApplicationsViewMode(
  value: unknown,
): ApplicationsViewMode {
  return value === "grid" ? "grid" : DEFAULT_APPLICATIONS_VIEW_MODE;
}

export function formatLabel(format: PackageFormat) {
  if (format === "appimage") return "AppImage";
  if (format === "local") return "Local";
  return format.toUpperCase();
}

export function scopeLabel(item: ApplicationInstallation) {
  if (item.format === "appimage") return "Fichier portable";
  if (item.format === "local") {
    return item.scope === "user"
      ? "Installation locale"
      : "Installation manuelle";
  }
  if (item.scope === "user") return "Utilisateur";
  if (item.installation && !["system", "default"].includes(item.installation)) {
    return `Système · ${item.installation}`;
  }
  return "Système";
}

export function installationSize(
  item: ApplicationInstallation,
  exactSize: (value: number | null) => string,
) {
  return item.installedSize !== null
    ? exactSize(item.installedSize)
    : item.reportedSize || "Non disponible";
}

function searchable(value: string) {
  return value
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLocaleLowerCase("fr");
}

export function filterInstallations(
  items: ApplicationInstallation[],
  options: {
    query: string;
    format: string;
    scope: string;
    technical: boolean;
  },
) {
  const query = searchable(options.query.trim());
  return items.filter(
    (item) =>
      (options.technical || item.isDesktopApplication) &&
      (options.format === "all" || options.format === item.format) &&
      (options.scope === "all" || options.scope === item.scope) &&
      searchable(`${item.name} ${item.nativeId}`).includes(query),
  );
}
