import { useState, type FormEvent } from "react";
import { FolderSearch, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

export function AppImageLocations({
  busy,
  enabled,
  locations,
  extraPaths,
  onAdd,
  onRemove,
}: {
  busy: boolean;
  enabled: boolean;
  locations: string[];
  extraPaths: string[];
  onAdd: (path: string) => Promise<string | null>;
  onRemove: (path: string) => Promise<void>;
}) {
  const [path, setPath] = useState("");
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    const failure = await onAdd(path);
    setError(failure);
    if (!failure) setPath("");
  };

  return (
    <section className="inventory-panel settings-panel">
      <h2>Emplacements AppImage</h2>
      <p className="mt-2 text-sm text-muted-foreground">
        Ajoute le chemin d’un fichier ou d’un dossier. Les fichiers sont
        examinés sans être lancés.
      </p>
      <form onSubmit={(event) => void submit(event)} className="mt-5">
        <label htmlFor="appimage-path" className="text-xs font-medium">
          Fichier ou dossier à analyser
        </label>
        <div className="mt-2 flex flex-wrap gap-2">
          <Input
            id="appimage-path"
            className="min-w-48 flex-1"
            placeholder="~/Applications ou /chemin/logiciel.AppImage"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            aria-describedby="appimage-help"
            required
            disabled={busy || !enabled}
          />
          <Button type="submit" disabled={busy || !enabled}>
            <FolderSearch />
            {busy ? "Analyse…" : "Analyser"}
          </Button>
        </div>
        <p id="appimage-help" className="mt-2 text-xs text-muted-foreground">
          Copie le chemin depuis le gestionnaire de fichiers. Les dossiers
          configurés sont conservés dans vos préférences locales.
        </p>
        {error && (
          <p role="alert" className="provider-error">
            {error}
          </p>
        )}
      </form>
      {extraPaths.length > 0 && (
        <div className="mt-5">
          <h3 className="text-xs font-medium">Emplacements ajoutés</h3>
          <ul className="mt-2 space-y-2">
            {extraPaths.map((value) => (
              <li
                key={value}
                className="flex items-center justify-between gap-3 rounded-md bg-muted px-3 py-1 text-xs"
              >
                <span className="break-all">{value}</span>
                <Button
                  variant="ghost"
                  size="icon"
                  disabled={busy}
                  aria-label={`Retirer l’emplacement ${value} de l’analyse`}
                  onClick={() => void onRemove(value)}
                >
                  <X />
                </Button>
              </li>
            ))}
          </ul>
        </div>
      )}
      <details className="mt-5 text-xs text-muted-foreground">
        <summary className="cursor-pointer">
          Emplacements pris en compte ({locations.length})
        </summary>
        <ul className="mt-3 space-y-2">
          {locations.map((value) => (
            <li className="break-all" key={value}>
              {value}
            </li>
          ))}
        </ul>
        {locations.length === 0 && (
          <p className="mt-3">Aucun emplacement analysé pour le moment.</p>
        )}
      </details>
    </section>
  );
}
