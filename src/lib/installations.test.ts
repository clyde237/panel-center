import assert from "node:assert/strict";
import { test } from "node:test";
import type { ApplicationInstallation } from "./contracts.ts";
import {
  DEFAULT_APPLICATIONS_VIEW_MODE,
  filterInstallations,
  formatLabel,
  resolveApplicationsViewMode,
  scopeLabel,
  installationSize,
} from "./installations.ts";

function item(
  overrides: Partial<ApplicationInstallation>,
): ApplicationInstallation {
  return {
    id: "flatpak:user:org.test.Editor/x86_64/stable",
    name: "Éditeur",
    nativeId: "org.test.Editor",
    version: "1",
    format: "flatpak",
    scope: "user",
    description: "",
    installedSize: null,
    reportedSize: "12 MB",
    installation: "user",
    reference: "app/org.test.Editor/x86_64/stable",
    path: null,
    isExecutable: null,
    isDesktopApplication: true,
    canUninstall: false,
    unavailableReason: "Lecture seule",
    ...overrides,
  };
}

test("search ignores accents and finds native identifiers", () => {
  const values = [item({})];
  const options = {
    query: "editeur",
    format: "all",
    scope: "all",
    technical: false,
  };
  assert.equal(filterInstallations(values, options).length, 1);
  assert.equal(
    filterInstallations(values, { ...options, query: "org.test" }).length,
    1,
  );
});

test("scope and format filters preserve distinct installations of the same app", () => {
  const values = [
    item({}),
    item({
      id: "flatpak:system:extra:org.test.Editor",
      scope: "system",
      installation: "extra",
    }),
    item({ id: "deb:editor", format: "deb", scope: "system" }),
  ];
  const options = {
    query: "",
    format: "flatpak",
    scope: "all",
    technical: false,
  };
  assert.equal(filterInstallations(values, options).length, 2);
  assert.equal(
    filterInstallations(values, { ...options, scope: "system" })[0]
      .installation,
    "extra",
  );
  assert.equal(scopeLabel(values[1]), "Système · extra");
});

test("technical packages remain opt-in and portable files remain visible", () => {
  const values = [
    item({ isDesktopApplication: false }),
    item({ id: "appimage:/tmp/file", format: "appimage", path: "/tmp/file" }),
  ];
  const options = { query: "", format: "all", scope: "all", technical: false };
  assert.equal(filterInstallations(values, options).length, 1);
  assert.equal(
    filterInstallations(values, { ...options, technical: true }).length,
    2,
  );
  assert.equal(scopeLabel(values[1]), "Fichier portable");
});

test("reported Flatpak sizes are displayed without inventing exact byte counts", () => {
  assert.equal(
    installationSize(item({}), () => "exact"),
    "12 MB",
  );
  assert.equal(
    installationSize(item({ reportedSize: null }), () => "exact"),
    "Non disponible",
  );
  assert.equal(
    installationSize(item({ installedSize: 1024 }), () => "exact"),
    "exact",
  );
});

test("applications view defaults to list and accepts grid", () => {
  assert.equal(DEFAULT_APPLICATIONS_VIEW_MODE, "list");
  assert.equal(resolveApplicationsViewMode(undefined), "list");
  assert.equal(resolveApplicationsViewMode("list"), "list");
  assert.equal(resolveApplicationsViewMode("grid"), "grid");
  assert.equal(resolveApplicationsViewMode("unknown"), "list");
});

test("local connector format and scope labels distinguish user and manual installs", () => {
  assert.equal(formatLabel("local"), "Local");
  assert.equal(formatLabel("appimage"), "AppImage");
  assert.equal(formatLabel("deb"), "DEB");

  const localUser = item({
    id: "local:user:/home/user/.local/share/applications/com.abdownloadmanager.desktop",
    name: "AB Download Manager",
    nativeId: "com.abdownloadmanager",
    format: "local",
    scope: "user",
  });
  const localSystem = item({
    id: "local:system:/home/user/.local/share/applications/antigravity.desktop",
    name: "Antigravity",
    nativeId: "antigravity",
    format: "local",
    scope: "system",
  });

  assert.equal(scopeLabel(localUser), "Installation locale");
  assert.equal(scopeLabel(localSystem), "Installation manuelle");
  assert.equal(
    filterInstallations([localUser, localSystem], {
      query: "ab download",
      format: "local",
      scope: "user",
      technical: false,
    }).length,
    1,
  );
});
