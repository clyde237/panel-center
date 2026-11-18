import { useEffect, useState } from "react";
import { HardDrive, Monitor, Palette, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { AppImageLocations } from "@/features/applications/appimage-locations";
import type { Inventory, Preferences, SystemInfo } from "@/lib/contracts";
import {
  clearHistory,
  getPreferences,
  isDesktop,
  updatePreferences,
} from "@/lib/desktop";
import { applyTheme } from "@/lib/theme";

interface SettingsViewProps {
  system: SystemInfo | null;
  inventory: Inventory | null;
  loading: boolean;
  error: string | null;
  extraPaths: string[];
  addLocation: (path: string) => Promise<string | null>;
  removeLocation: (path: string) => Promise<void>;
}

export function SettingsView({
  system,
  inventory,
  loading,
  error,
  extraPaths,
  addLocation,
  removeLocation,
}: SettingsViewProps) {
  const [prefs, setPrefs] = useState<Preferences>({
    theme: "system",
    appimageFolders: [],
    historyRetentionDays: 30,
  });
  const [saved, setSaved] = useState(false);
  const [historyCleared, setHistoryCleared] = useState(false);

  useEffect(() => {
    if (isDesktop()) {
      getPreferences()
        .then((p) => {
          setPrefs(p);
        })
        .catch(() => {});
    }
  }, []);

  const handleSave = (updated: Preferences) => {
    setPrefs(updated);
    applyTheme(updated.theme);
    if (isDesktop()) {
      updatePreferences(updated)
        .then(() => {
          setSaved(true);
          setTimeout(() => setSaved(false), 2000);
        })
        .catch(() => {});
    }
  };

  const handleClearHistory = () => {
    if (confirm("Voulez-vous vraiment effacer l’historique des opérations ?")) {
      clearHistory()
        .then(() => {
          setHistoryCleared(true);
          setTimeout(() => setHistoryCleared(false), 2500);
        })
        .catch(() => {});
    }
  };

  const formats = ["deb", "snap", "flatpak", "appimage"] as const;
  const displayedExtraPaths = Array.from(
    new Set([...prefs.appimageFolders, ...extraPaths]),
  );

  return (
    <div className="space-y-6">
      <AppImageLocations
        busy={loading}
        enabled={isDesktop()}
        locations={inventory?.appimageLocations ?? []}
        extraPaths={displayedExtraPaths}
        onAdd={async (path) => {
          const trimmed = path.trim();
          if (!trimmed) return "Renseigne un chemin de fichier ou de dossier.";
          if (!prefs.appimageFolders.includes(trimmed)) {
            const nextFolders = [...prefs.appimageFolders, trimmed];
            handleSave({ ...prefs, appimageFolders: nextFolders });
          }
          return await addLocation(trimmed);
        }}
        onRemove={async (path) => {
          const nextFolders = prefs.appimageFolders.filter((p) => p !== path);
          handleSave({ ...prefs, appimageFolders: nextFolders });
          await removeLocation(path);
        }}
      />

      {error && (
        <p role="alert" className="provider-error">
          {error}
        </p>
      )}

      <section className="inventory-panel settings-panel">
        <h2 className="flex items-center gap-2">
          <Palette size={18} />
          Préférences de l’application
        </h2>

        <div className="mt-4 grid gap-4 sm:grid-cols-2">
          <div className="space-y-1.5">
            <label
              htmlFor="theme-select"
              className="text-xs font-semibold text-muted-foreground uppercase tracking-wider"
            >
              Apparence
            </label>
            <select
              id="theme-select"
              className="format-select w-full"
              value={prefs.theme}
              onChange={(e) =>
                handleSave({
                  ...prefs,
                  theme: e.target.value as Preferences["theme"],
                })
              }
            >
              <option value="system">Thème système Ubuntu</option>
              <option value="light">Clair</option>
              <option value="dark">Sombre</option>
            </select>
          </div>

          <div className="space-y-1.5">
            <label
              htmlFor="retention-select"
              className="text-xs font-semibold text-muted-foreground uppercase tracking-wider"
            >
              Conservation de l’historique
            </label>
            <select
              id="retention-select"
              className="format-select w-full"
              value={prefs.historyRetentionDays}
              onChange={(e) =>
                handleSave({
                  ...prefs,
                  historyRetentionDays: Number(e.target.value),
                })
              }
            >
              <option value={30}>Conserver 30 jours</option>
              <option value={90}>Conserver 90 jours</option>
              <option value={0}>Conservation illimitée</option>
            </select>
          </div>
        </div>

        <div className="mt-5 flex items-center justify-between border-t pt-4">
          <Button
            variant="outline"
            size="sm"
            className="text-destructive hover:text-destructive"
            onClick={handleClearHistory}
          >
            <Trash2 size={14} />
            Purger l’historique maintenant
          </Button>
          <div className="flex items-center gap-3">
            {historyCleared && (
              <span className="text-xs text-emerald-600 font-medium">
                Historique purgé
              </span>
            )}
            {saved && (
              <span className="text-xs text-emerald-600 font-medium">
                Préférences enregistrées
              </span>
            )}
          </div>
        </div>
      </section>

      <section className="inventory-panel settings-panel">
        <h2 className="flex items-center gap-2">
          <Monitor size={18} />
          Environnement
        </h2>
        <dl className="detail-fields mt-4">
          <div>
            <dt>Système</dt>
            <dd>{system?.os ?? "Accessible dans l’application desktop"}</dd>
          </div>
          <div>
            <dt>Architecture</dt>
            <dd>{system?.architecture ?? "Non disponible"}</dd>
          </div>
          <div>
            <dt>Version de Panel Center</dt>
            <dd>{system?.appVersion ?? "0.1.0"}</dd>
          </div>
          <div>
            <dt>Mode d’exécution</dt>
            <dd>{system?.mode ?? "Local"}</dd>
          </div>
        </dl>

        <h2 className="mt-8 flex items-center gap-2">
          <HardDrive size={18} />
          Connecteurs
        </h2>
        <div className="mt-4 space-y-4">
          {(
            inventory?.providers ??
            formats.map((key) => ({
              format: key,
              message: "Inventaire accessible dans la version desktop.",
            }))
          ).map((p) => (
            <div key={p.format} className="provider-setting">
              <strong>
                {p.format === "appimage" ? "AppImage" : p.format.toUpperCase()}
              </strong>
              <p>{p.message}</p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
