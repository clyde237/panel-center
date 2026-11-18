import { useEffect, useState } from "react";
import {
  AlertCircle,
  Box,
  ChevronRight,
  FolderSearch,
  History,
  Info,
  LayoutGrid,
  List,
  LoaderCircle,
  Monitor,
  Package,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ApplicationsView } from "@/features/applications/applications-view";
import { InstallationDetails } from "@/features/applications/installation-details";
import { UninstallDialog } from "@/features/uninstall/uninstall-dialog";
import { HistoryView } from "@/features/history/history-view";
import { SettingsView } from "@/features/settings/settings-view";
import { useInventory } from "@/features/applications/use-inventory";
import type { ApplicationInstallation, SystemInfo } from "@/lib/contracts";
import { getPreferences, getSystemInfo, isDesktop } from "@/lib/desktop";
import { applyTheme } from "@/lib/theme";
import { cn } from "@/lib/utils";
import {
  DEFAULT_APPLICATIONS_VIEW_MODE,
  filterInstallations,
  type ApplicationsViewMode,
} from "@/lib/installations";

type Page = "applications" | "history" | "settings";
const pages = {
  applications: "Applications",
  history: "Historique",
  settings: "Paramètres",
};
const formats = ["deb", "snap", "flatpak", "appimage"] as const;

export function App() {
  const [page, setPage] = useState<Page>("applications");
  const [query, setQuery] = useState("");
  const [format, setFormat] = useState("all");
  const [scope, setScope] = useState("all");
  const [technical, setTechnical] = useState(false);
  const [viewMode, setViewMode] = useState<ApplicationsViewMode>(
    DEFAULT_APPLICATIONS_VIEW_MODE,
  );
  const [selection, setSelection] = useState<ApplicationInstallation | null>(
    null,
  );
  const [uninstallTarget, setUninstallTarget] =
    useState<ApplicationInstallation | null>(null);
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const {
    inventory,
    loading,
    error,
    refresh,
    extraPaths,
    addLocation,
    removeLocation,
  } = useInventory();

  useEffect(() => {
    void getSystemInfo()
      .then(setSystem)
      .catch(() => {});
    void getPreferences()
      .then((p) => applyTheme(p.theme))
      .catch(() => {});
  }, []);
  useEffect(() => {
    document.title = `Panel Center · ${pages[page]}`;
  }, [page]);

  const all = inventory?.installations ?? [];
  const filtered = filterInstallations(all, {
    query,
    format,
    scope,
    technical,
  });
  const hasFilters = Boolean(query || format !== "all" || scope !== "all");
  const partial = inventory?.providers.some(
    (provider) => provider.state !== "ready",
  );

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Aller au contenu
      </a>
      <aside className="sidebar">
        <div className="brand">
          <img src="/panel-center.svg" width="34" height="34" alt="" />
          <div>
            Panel Center<span>Applications Ubuntu</span>
          </div>
        </div>
        <nav aria-label="Navigation principale" className="mt-10 space-y-1">
          {(
            [
              ["applications", LayoutGrid],
              ["history", History],
              ["settings", Settings2],
            ] as const
          ).map(([key, Icon]) => (
            <button
              key={key}
              onClick={() => setPage(key)}
              className={cn("nav-item", page === key && "nav-item-active")}
              aria-current={page === key ? "page" : undefined}
            >
              <Icon size={18} aria-hidden="true" />
              <span>{pages[key]}</span>
              {key === "applications" && inventory && (
                <span className="nav-count">
                  {all.filter((item) => item.isDesktopApplication).length}
                </span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <Monitor size={17} aria-hidden="true" />
          <div>
            <strong>{system?.os ?? "Poste local"}</strong>
            <span>{system?.architecture ?? "Version desktop requise"}</span>
          </div>
        </div>
      </aside>

      <main id="main-content" className="main-panel">
        <header className="topbar">
          <span>
            Mon ordinateur <ChevronRight size={14} aria-hidden="true" />{" "}
            <strong>{pages[page]}</strong>
          </span>
          <span className="local-indicator">
            <span />
            {isDesktop() ? "Local" : "Aperçu navigateur"}
          </span>
        </header>
        <div className="page-content">
          <div className="page-heading">
            <div>
              <p className="eyebrow">VOTRE ESPACE LOGICIEL</p>
              <h1>{pages[page]}</h1>
              <p className="page-description">
                {page === "applications"
                  ? "Retrouve les logiciels installés sur ton ordinateur."
                  : page === "history"
                    ? "Les opérations effectuées depuis Panel Center."
                    : "L’environnement et les connecteurs de ton application."}
              </p>
            </div>
            {page === "applications" && (
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" onClick={() => setPage("settings")}>
                  <FolderSearch />
                  AppImage
                </Button>
                <Button
                  variant="outline"
                  disabled={loading || !isDesktop()}
                  onClick={() => void refresh()}
                >
                  <RefreshCw className={cn(loading && "animate-spin")} />
                  Actualiser
                </Button>
              </div>
            )}
          </div>

          {page === "applications" && (
            <>
              <div className="mode-notice">
                <ShieldCheck size={20} aria-hidden="true" />
                <div>
                  <strong>Gestionnaire d’applications locales</strong>
                  <p>
                    Retrouvez vos logiciels installés (DEB, Snap, Flatpak,
                    AppImage) et consultez les conséquences détaillées avant
                    toute désinstallation.
                  </p>
                </div>
              </div>
              <div className="format-strip" aria-label="État des connecteurs">
                {formats.map((key) => {
                  const status = inventory?.providers.find(
                    (p) => p.format === key,
                  );
                  return (
                    <div className="format-status" key={key}>
                      <Box size={17} aria-hidden="true" />
                      <span>
                        {key === "appimage" ? "AppImage" : key.toUpperCase()}
                      </span>
                      <small
                        className={cn(
                          status?.state === "ready" && "status-ready",
                        )}
                      >
                        {status?.state === "ready"
                          ? "Connecté"
                          : status?.state === "partial"
                            ? "Partiel"
                            : status?.state === "error"
                              ? "Erreur"
                              : status?.state === "unavailable"
                                ? "Indisponible"
                                : "En attente"}
                      </small>
                    </div>
                  );
                })}
              </div>
              {partial && (
                <p className="mb-4 text-xs text-muted-foreground" role="status">
                  L’inventaire est partiel. Consulte les précisions de chaque
                  connecteur sous la liste.
                </p>
              )}
              {loading && inventory && (
                <p className="mb-4 text-xs text-muted-foreground" role="status">
                  Actualisation en cours. Les derniers résultats restent
                  affichés.
                </p>
              )}
              <section
                className="inventory-panel"
                aria-label="Inventaire des installations"
                aria-busy={loading}
              >
                <div className="inventory-toolbar">
                  <div className="search-field">
                    <Search size={17} aria-hidden="true" />
                    <Input
                      aria-label="Rechercher une application"
                      placeholder="Rechercher une application…"
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                    />
                  </div>
                  <select
                    className="format-select"
                    aria-label="Filtrer par format"
                    value={format}
                    onChange={(e) => setFormat(e.target.value)}
                  >
                    <option value="all">Tous les formats</option>
                    {formats.map((key) => (
                      <option key={key} value={key}>
                        {key === "appimage" ? "AppImage" : key.toUpperCase()}
                      </option>
                    ))}
                  </select>
                  <select
                    className="format-select"
                    aria-label="Filtrer par portée"
                    value={scope}
                    onChange={(event) => setScope(event.target.value)}
                  >
                    <option value="all">Toutes les portées</option>
                    <option value="user">Utilisateur</option>
                    <option value="system">Système</option>
                  </select>
                  <div
                    className="view-toggle"
                    role="group"
                    aria-label="Mode d’affichage"
                  >
                    <button
                      type="button"
                      className={cn(
                        "view-toggle-btn",
                        viewMode === "list" && "view-toggle-btn-active",
                      )}
                      aria-pressed={viewMode === "list"}
                      onClick={() => setViewMode("list")}
                    >
                      <List size={15} aria-hidden="true" />
                      <span>Liste</span>
                    </button>
                    <button
                      type="button"
                      className={cn(
                        "view-toggle-btn",
                        viewMode === "grid" && "view-toggle-btn-active",
                      )}
                      aria-pressed={viewMode === "grid"}
                      onClick={() => setViewMode("grid")}
                    >
                      <LayoutGrid size={15} aria-hidden="true" />
                      <span>Grille</span>
                    </button>
                  </div>
                </div>
                <div className="list-meta">
                  <span aria-live="polite">
                    {inventory
                      ? `${filtered.length} installation${filtered.length > 1 ? "s" : ""}`
                      : "Inventaire local"}
                  </span>
                  <label>
                    <input
                      type="checkbox"
                      checked={technical}
                      onChange={(e) => setTechnical(e.target.checked)}
                    />
                    Inclure les paquets techniques
                  </label>
                </div>
                {loading && !inventory ? (
                  <div className="empty-state" role="status">
                    <LoaderCircle className="animate-spin" />
                    <h2>Lecture des applications</h2>
                    <p>
                      Lecture des gestionnaires et des emplacements AppImage…
                    </p>
                  </div>
                ) : error ? (
                  <div className="empty-state" role="alert">
                    <AlertCircle />
                    <h2>
                      {isDesktop()
                        ? "Inventaire indisponible"
                        : "Ouvre l’application desktop"}
                    </h2>
                    <p>{error}</p>
                    {isDesktop() && (
                      <Button variant="outline" onClick={() => void refresh()}>
                        Réessayer
                      </Button>
                    )}
                  </div>
                ) : filtered.length === 0 ? (
                  <div className="empty-state">
                    <Package />
                    <h2>
                      {hasFilters
                        ? "Aucune correspondance"
                        : "Aucune application détectée"}
                    </h2>
                    <p>
                      {hasFilters
                        ? "Essaie un autre nom ou un autre format."
                        : "Tu peux inclure les paquets techniques pour élargir la liste."}
                    </p>
                    {hasFilters && (
                      <Button
                        variant="outline"
                        onClick={() => {
                          setQuery("");
                          setFormat("all");
                          setScope("all");
                        }}
                      >
                        Effacer les filtres
                      </Button>
                    )}
                  </div>
                ) : (
                  <>
                    <ApplicationsView
                      items={filtered}
                      viewMode={viewMode}
                      onSelect={setSelection}
                    />
                    <div className="list-footer">
                      <Info size={14} aria-hidden="true" />
                      Les tailles déclarées incluent des composants qui peuvent
                      être partagés.
                    </div>
                  </>
                )}
              </section>
              {inventory?.providers
                .filter(
                  (p) =>
                    p.state === "error" ||
                    p.state === "unavailable" ||
                    p.state === "partial",
                )
                .map((p) => (
                  <p className="provider-error" role="status" key={p.format}>
                    {p.message}
                  </p>
                ))}
            </>
          )}

          {page === "history" && <HistoryView />}

          {page === "settings" && (
            <SettingsView
              system={system}
              inventory={inventory}
              loading={loading}
              error={error}
              extraPaths={extraPaths}
              addLocation={addLocation}
              removeLocation={removeLocation}
            />
          )}
          <footer className="page-footer">
            <span>
              Panel Center <span className="font-mono">0.1.0</span>
            </span>
            <span>Vos applications, sur votre ordinateur.</span>
          </footer>
        </div>
      </main>
      <InstallationDetails
        installation={selection}
        onClose={() => setSelection(null)}
        onRequestUninstall={(item) => setUninstallTarget(item)}
      />
      <UninstallDialog
        installation={uninstallTarget}
        onClose={() => setUninstallTarget(null)}
        onSuccess={() => void refresh()}
      />
    </div>
  );
}
