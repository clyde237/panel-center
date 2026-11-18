import { ArrowUpRight, Package } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ApplicationInstallation } from "@/lib/contracts";
import {
  installationSize,
  scopeLabel,
  type ApplicationsViewMode,
} from "@/lib/installations";
import { formatSize } from "@/lib/utils";

interface ApplicationsViewProps {
  items: ApplicationInstallation[];
  viewMode: ApplicationsViewMode;
  onSelect: (item: ApplicationInstallation) => void;
}

export function ApplicationsView({
  items,
  viewMode,
  onSelect,
}: ApplicationsViewProps) {
  if (viewMode === "grid") {
    return (
      <div className="grid-scroll">
        <ul
          className="applications-grid"
          role="list"
          aria-label="Applications en grille"
        >
          {items.map((item) => (
            <li key={item.id} className="applications-grid-item">
              <article className="app-card">
                <header className="app-card-header">
                  <span className="app-icon">
                    <Package size={20} aria-hidden="true" />
                  </span>
                  <div className="app-card-heading">
                    <h2>
                      <button
                        type="button"
                        className="app-card-trigger"
                        onClick={() => onSelect(item)}
                      >
                        {item.name}
                      </button>
                    </h2>
                    <p className="app-card-version">
                      {item.version || "Version non disponible"}
                    </p>
                  </div>
                </header>
                <div className="app-card-meta">
                  <span className="format-badge">
                    {item.format.toUpperCase()}
                  </span>
                  <span className="scope-label">{scopeLabel(item)}</span>
                </div>
                <footer className="app-card-footer">
                  <span className="size-cell">
                    {installationSize(item, formatSize)}
                  </span>
                  <span className="app-card-action-cue" aria-hidden="true">
                    <ArrowUpRight size={16} />
                  </span>
                </footer>
              </article>
            </li>
          ))}
        </ul>
      </div>
    );
  }

  return (
    <div className="table-scroll">
      <table className="applications-table">
        <thead>
          <tr>
            <th scope="col">Application</th>
            <th scope="col">Format</th>
            <th scope="col">Taille installée</th>
            <th scope="col">
              <span className="sr-only">Détails</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.id}>
              <td>
                <button
                  type="button"
                  className="application-name"
                  onClick={() => onSelect(item)}
                >
                  <span className="app-icon">
                    <Package size={20} aria-hidden="true" />
                  </span>
                  <span>
                    <strong>{item.name}</strong>
                    <small>{item.version || "Version non disponible"}</small>
                  </span>
                </button>
              </td>
              <td>
                <span className="format-badge">
                  {item.format.toUpperCase()}
                </span>
                <span className="scope-label">{scopeLabel(item)}</span>
              </td>
              <td className="size-cell">
                {installationSize(item, formatSize)}
              </td>
              <td>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Voir les détails de ${item.name}`}
                  onClick={() => onSelect(item)}
                >
                  <ArrowUpRight />
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
