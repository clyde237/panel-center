import assert from "node:assert/strict";
import { test } from "node:test";
import { DesktopRequiredError, errorMessage } from "./desktop.ts";
import type { UninstallPlan } from "./contracts.ts";
import { applyTheme } from "./theme.ts";

test("errorMessage formats string, Error, and AppError correctly", () => {
  assert.equal(errorMessage(new Error("Erreur système")), "Erreur système");
  assert.equal(
    errorMessage({ code: "FORBIDDEN", message: "Accès refusé" }),
    "Accès refusé",
  );
  assert.equal(
    errorMessage(null),
    "Une erreur inattendue est survenue. Réessaie dans quelques instants.",
  );
  assert.equal(
    errorMessage(123),
    "Une erreur inattendue est survenue. Réessaie dans quelques instants.",
  );
});

test("DesktopRequiredError provides explicit user-friendly notice", () => {
  const err = new DesktopRequiredError();
  assert.match(err.message, /Ouvre la version desktop/);
});

test("UninstallPlan contract validates blocked state and consequences", () => {
  const plan: UninstallPlan = {
    id: "plan_deb_dpkg_12345",
    targetId: "deb:system:dpkg:amd64",
    targetName: "dpkg",
    format: "deb",
    scope: "system",
    nativeId: "dpkg",
    options: {
      purgeConfig: true,
      removeData: false,
    },
    affectedItems: ["Paquet DEB : dpkg"],
    dataImpact: "Le paquet sera supprimé avec ses fichiers de configuration.",
    estimatedFreedSpace: "1.2 MB",
    requiresAuth: true,
    isBlocked: true,
    blockedReason: "Paquet système protégé.",
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.isBlocked, true);
  assert.equal(plan.blockedReason, "Paquet système protégé.");
  assert.equal(plan.requiresAuth, true);
  assert.equal(plan.affectedItems.length, 1);
});

test("UninstallPlan contract accepts optional targetPath", () => {
  const plan: UninstallPlan = {
    id: "plan_appimage_test",
    targetId: "appimage:/home/user/App.AppImage",
    targetName: "App",
    format: "appimage",
    scope: "user",
    nativeId: "app",
    targetPath: "/home/user/App.AppImage",
    options: {
      purgeConfig: false,
      removeData: false,
    },
    affectedItems: ["Fichier exécutable : /home/user/App.AppImage"],
    dataImpact: "Suppression du fichier.",
    estimatedFreedSpace: "50 MB",
    requiresAuth: false,
    isBlocked: false,
    blockedReason: null,
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.targetPath, "/home/user/App.AppImage");
  assert.equal(plan.isBlocked, false);
});

test("applyTheme handles node environment gracefully without throwing", () => {
  assert.doesNotThrow(() => applyTheme("light"));
  assert.doesNotThrow(() => applyTheme("dark"));
  assert.doesNotThrow(() => applyTheme("system"));
});

test("applyTheme toggles .dark class on documentElement when DOM is present", () => {
  const classes = new Set<string>();
  const mockDoc = {
    documentElement: {
      classList: {
        add: (cls: string) => classes.add(cls),
        remove: (cls: string) => classes.delete(cls),
        contains: (cls: string) => classes.has(cls),
      },
    },
  };
  (globalThis as unknown as { document: unknown }).document = mockDoc;

  try {
    applyTheme("dark");
    assert.equal(classes.has("dark"), true);

    applyTheme("light");
    assert.equal(classes.has("dark"), false);
  } finally {
    delete (globalThis as unknown as { document?: unknown }).document;
  }
});

test("Preferences AppImage folders deduplication and merging preserves uniqueness", () => {
  const persistedFolders = ["/opt/apps", "~/Applications"];
  const sessionFolders = ["/opt/apps", "/media/user/USB/apps"];
  const merged = Array.from(new Set([...persistedFolders, ...sessionFolders]));

  assert.equal(merged.length, 3);
  assert.deepEqual(merged, [
    "/opt/apps",
    "~/Applications",
    "/media/user/USB/apps",
  ]);
});

test("UninstallPlan contract accepts Flatpak reference in targetPath", () => {
  const plan: UninstallPlan = {
    id: "plan_flatpak_vlc_beta",
    targetId: "flatpak:user:default:app/org.videolan.VLC/x86_64/beta",
    targetName: "VLC Beta",
    format: "flatpak",
    scope: "user",
    nativeId: "org.videolan.VLC",
    targetPath: "app/org.videolan.VLC/x86_64/beta",
    options: {
      purgeConfig: false,
      removeData: true,
    },
    affectedItems: ["Flatpak (user) : app/org.videolan.VLC/x86_64/beta"],
    dataImpact:
      "L’application Flatpak et ses données utilisateur stockées dans ~/.var/app seront supprimées.",
    estimatedFreedSpace: "150 MB",
    requiresAuth: false,
    isBlocked: false,
    blockedReason: null,
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.targetPath, "app/org.videolan.VLC/x86_64/beta");
  assert.equal(plan.format, "flatpak");
  assert.equal(plan.scope, "user");
  assert.equal(plan.isBlocked, false);
});

test("UninstallPlan handles blocked state for nonexistent or dangerous AppImage paths", () => {
  const plan: UninstallPlan = {
    id: "plan_appimage_missing",
    targetId: "appimage:/tmp/missing.AppImage",
    targetName: "Missing App",
    format: "appimage",
    scope: "user",
    nativeId: "missing-app",
    targetPath: "/tmp/missing.AppImage",
    options: {
      purgeConfig: false,
      removeData: false,
    },
    affectedItems: ["Fichier exécutable : /tmp/missing.AppImage"],
    dataImpact: "Le fichier autonome AppImage sera supprimé du disque.",
    estimatedFreedSpace: null,
    requiresAuth: false,
    isBlocked: true,
    blockedReason:
      "Le fichier AppImage est introuvable à cet emplacement (fichier déplacé ou support déconnecté).",
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.isBlocked, true);
  assert.match(plan.blockedReason ?? "", /introuvable/);
});

test("Validation rejects flag-like or malformed application identifiers", () => {
  const isMalformedOrFlag = (id: string) =>
    id.startsWith("-") || /\s/.test(id) || id.includes("\0");

  assert.equal(isMalformedOrFlag("--all"), true);
  assert.equal(isMalformedOrFlag("-y"), true);
  assert.equal(isMalformedOrFlag("app && rm -rf /"), true);
  assert.equal(isMalformedOrFlag("app\0null"), true);
  assert.equal(isMalformedOrFlag("org.videolan.VLC"), false);
  assert.equal(isMalformedOrFlag("app/org.videolan.VLC/x86_64/beta"), false);
  assert.equal(isMalformedOrFlag("vlc:amd64"), false);
});

test("Preferences folder limit and valid character validation", () => {
  const isValidAppImageFolder = (folder: string) =>
    folder.length > 0 && folder.length <= 4096 && !folder.includes("\0");

  assert.equal(isValidAppImageFolder("/home/user/Applications"), true);
  assert.equal(isValidAppImageFolder(""), false);
  assert.equal(isValidAppImageFolder("/path/with/\0null"), false);
});

test("Preferences theme and retention value validation", () => {
  const isValidTheme = (theme: string) =>
    theme === "system" || theme === "light" || theme === "dark";
  const isValidRetention = (days: number) =>
    Number.isInteger(days) && days >= 0 && days <= 3650;

  assert.equal(isValidTheme("system"), true);
  assert.equal(isValidTheme("light"), true);
  assert.equal(isValidTheme("dark"), true);
  assert.equal(isValidTheme("neon"), false);

  assert.equal(isValidRetention(0), true);
  assert.equal(isValidRetention(30), true);
  assert.equal(isValidRetention(90), true);
  assert.equal(isValidRetention(99999), false);
  assert.equal(isValidRetention(-1), false);
});

test("Protected vital Ubuntu package contract includes kernel and grub", () => {
  const isVitalDeb = (id: string) => {
    const norm = id.split(":")[0].trim();
    const staticList = [
      "apt",
      "dpkg",
      "systemd",
      "ubuntu-desktop",
      "panel-center",
      "grub-common",
      "linux-image-generic",
      "polkitd",
    ];
    return (
      staticList.includes(norm) ||
      norm.startsWith("linux-image-") ||
      norm.startsWith("linux-modules-")
    );
  };

  assert.equal(isVitalDeb("apt"), true);
  assert.equal(isVitalDeb("systemd"), true);
  assert.equal(isVitalDeb("grub-common"), true);
  assert.equal(isVitalDeb("linux-image-6.8.0-45-generic"), true);
  assert.equal(isVitalDeb("linux-modules-6.8.0-45-generic"), true);
  assert.equal(isVitalDeb("vlc"), false);
  assert.equal(isVitalDeb("firefox"), false);
});

test("UninstallPlan supports named Flatpak system installation and Polkit auth flag", () => {
  const plan: UninstallPlan = {
    id: "plan_flatpak_sys_1",
    targetId: "flatpak:system:extra:app/org.videolan.VLC/x86_64/stable",
    targetName: "VLC",
    format: "flatpak",
    scope: "system",
    nativeId: "org.videolan.VLC",
    targetPath: "app/org.videolan.VLC/x86_64/stable",
    targetInstallation: "extra",
    options: {
      purgeConfig: false,
      removeData: false,
    },
    affectedItems: [
      "Flatpak (system · extra) : app/org.videolan.VLC/x86_64/stable",
    ],
    dataImpact: "L’application Flatpak sera désinstallée.",
    estimatedFreedSpace: "120 MB",
    requiresAuth: true,
    isBlocked: false,
    blockedReason: null,
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.requiresAuth, true);
  assert.equal(plan.targetInstallation, "extra");
  assert.equal(plan.isBlocked, false);
});

test("UninstallPlan carries full APT dependency preview breakdown", () => {
  const plan: UninstallPlan = {
    id: "plan_deb_vlc_1",
    targetId: "deb:system:vlc",
    targetName: "VLC",
    format: "deb",
    scope: "system",
    nativeId: "vlc",
    options: {
      purgeConfig: false,
      removeData: false,
    },
    affectedItems: ["Paquet DEB : vlc", "Dépendance retirée : vlc-plugin-qt"],
    debPreview: {
      targetPackage: "vlc",
      removedPackages: [
        {
          name: "vlc",
          version: "3.0.20-3",
          installedSize: 1024 * 500,
          description: "Lecteur multimédia",
          isTarget: true,
          isProtected: false,
          action: "remove",
        },
        {
          name: "vlc-plugin-qt",
          version: "3.0.20-3",
          installedSize: 1024 * 1200,
          description: "Interface Qt de VLC",
          isTarget: false,
          isProtected: false,
          action: "remove",
        },
      ],
      dependentPackages: [
        {
          name: "vlc-plugin-qt",
          version: "3.0.20-3",
          installedSize: 1024 * 1200,
          description: "Interface Qt de VLC",
          isTarget: false,
          isProtected: false,
          action: "remove",
        },
      ],
      newlyInstalledPackages: [],
      autoremoveCandidates: ["libvlc5"],
      blockingDependents: [],
      totalFreedBytes: 1024 * 1700,
    },
    dataImpact: "Le paquet sera retiré.",
    estimatedFreedSpace: "1,7 Mo",
    requiresAuth: true,
    isBlocked: false,
    blockedReason: null,
    createdAt: 1000,
    expiresAt: 1300,
  };

  assert.equal(plan.debPreview?.removedPackages.length, 2);
  assert.equal(plan.debPreview?.dependentPackages.length, 1);
  assert.deepEqual(plan.debPreview?.autoremoveCandidates, ["libvlc5"]);
  assert.equal(plan.debPreview?.totalFreedBytes, 1024 * 1700);
});
