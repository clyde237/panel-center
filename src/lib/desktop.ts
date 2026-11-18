import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  AppError,
  ApplicationInstallation,
  HistoryEntry,
  Inventory,
  Operation,
  Preferences,
  SystemInfo,
  UninstallPlan,
  UninstallPlanOptions,
} from "./contracts";

export class DesktopRequiredError extends Error {
  constructor() {
    super(
      "Ouvre la version desktop pour consulter les applications de ton PC.",
    );
  }
}

export const isDesktop = () => isTauri();

export function getInventory(appimagePaths: string[] = []): Promise<Inventory> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("list_installations", { appimagePaths });
}

export function getSystemInfo(): Promise<SystemInfo> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("get_system_info");
}

export function prepareUninstall(
  installation: ApplicationInstallation,
  options?: UninstallPlanOptions,
): Promise<UninstallPlan> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("prepare_uninstall", { installation, options });
}

export function confirmUninstall(planId: string): Promise<Operation> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("confirm_uninstall", { planId });
}

export function getOperation(): Promise<Operation | null> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("get_operation");
}

export function listHistory(): Promise<HistoryEntry[]> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("list_history");
}

export function clearHistory(): Promise<void> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("clear_history");
}

export function getPreferences(): Promise<Preferences> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("get_preferences");
}

export function updatePreferences(preferences: Preferences): Promise<void> {
  if (!isDesktop()) return Promise.reject(new DesktopRequiredError());
  return invoke("update_preferences", { preferences });
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === "object" &&
    error &&
    "message" in error &&
    typeof (error as AppError).message === "string"
  ) {
    return (error as AppError).message;
  }
  return "Une erreur inattendue est survenue. Réessaie dans quelques instants.";
}
