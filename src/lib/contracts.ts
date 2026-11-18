export type PackageFormat = "deb" | "snap" | "flatpak" | "appimage";

export interface ApplicationInstallation {
  id: string;
  name: string;
  nativeId: string;
  version: string;
  format: PackageFormat;
  scope: "system" | "user";
  description: string;
  installedSize: number | null;
  reportedSize: string | null;
  installation: string | null;
  reference: string | null;
  path: string | null;
  isExecutable: boolean | null;
  isDesktopApplication: boolean;
  canUninstall: boolean;
  unavailableReason: string;
}

export interface ProviderStatus {
  format: PackageFormat;
  state: "ready" | "partial" | "unavailable" | "planned" | "error";
  message: string;
}

export interface Inventory {
  installations: ApplicationInstallation[];
  providers: ProviderStatus[];
  generatedAt: number;
  appimageLocations: string[];
}

export interface UninstallPlanOptions {
  purgeConfig: boolean;
  removeData: boolean;
}

export interface DebAffectedPackage {
  name: string;
  version: string;
  installedSize: number | null;
  description: string;
  isTarget: boolean;
  isProtected: boolean;
  action: "remove" | "purge";
}

export interface DebDependencyPreview {
  targetPackage: string;
  removedPackages: DebAffectedPackage[];
  dependentPackages: DebAffectedPackage[];
  newlyInstalledPackages: string[];
  autoremoveCandidates: string[];
  blockingDependents: string[];
  totalFreedBytes: number | null;
}

export interface UninstallPlan {
  id: string;
  targetId: string;
  targetName: string;
  format: PackageFormat;
  scope: "system" | "user";
  nativeId: string;
  targetPath?: string | null;
  targetInstallation?: string | null;
  options: UninstallPlanOptions;
  affectedItems: string[];
  debPreview?: DebDependencyPreview | null;
  dataImpact: string;
  estimatedFreedSpace: string | null;
  requiresAuth: boolean;
  isBlocked: boolean;
  blockedReason: string | null;
  createdAt: number;
  expiresAt: number;
}

export type OperationStatus =
  "pending" | "running" | "verifying" | "completed" | "failed" | "cancelled";

export interface Operation {
  id: string;
  planId: string;
  targetId: string;
  targetName: string;
  format: PackageFormat;
  status: OperationStatus;
  step: string;
  progress: number | null;
  message: string;
  error: AppError | null;
  startedAt: number;
  finishedAt: number | null;
}

export interface HistoryEntry {
  id: string;
  operationId: string;
  targetName: string;
  nativeId: string;
  format: PackageFormat;
  status: OperationStatus;
  summary: string;
  diagnostic: string;
  timestamp: number;
}

export interface Preferences {
  theme: "system" | "light" | "dark";
  appimageFolders: string[];
  historyRetentionDays: number;
}

export interface SystemInfo {
  os: string;
  architecture: string;
  appVersion: string;
  mode: string;
}

export interface AppError {
  code: string;
  message: string;
}
