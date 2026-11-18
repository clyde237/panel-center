import { useEffect, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  History,
  Info,
  Package,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type { HistoryEntry } from "@/lib/contracts";
import { clearHistory, isDesktop, listHistory } from "@/lib/desktop";

export function HistoryView() {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedEntry, setSelectedEntry] = useState<HistoryEntry | null>(null);

  const fetchHistory = () => {
    if (!isDesktop()) {
      setLoading(false);
      return;
    }
    setLoading(true);
    listHistory()
      .then(setEntries)
      .catch(() => setEntries([]))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    fetchHistory();
  }, []);

  const handleClear = () => {
    if (
      !confirm("Voulez-vous vraiment effacer l’historique des opérations ?")
    ) {
      return;
    }
    clearHistory()
      .then(() => setEntries([]))
      .catch(() => {});
  };

  const formatDate = (timestamp: number) => {
    if (!timestamp) return "Date inconnue";
    const date = new Date(timestamp * 1000);
    return new Intl.DateTimeFormat("fr-FR", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(date);
  };

  if (loading) {
    return (
      <div className="empty-state py-12" role="status">
        <RefreshCw className="animate-spin text-primary" size={28} />
        <p className="text-sm">Chargement de l’historique…</p>
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <section className="inventory-panel empty-state">
        <History size={40} />
        <h2>Aucune opération enregistrée</h2>
        <p>
          Les opérations de désinstallation effectuées apparaîtront ici avec
          leur date, leur résultat et leur diagnostic technique.
        </p>
      </section>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-xs text-muted-foreground">
          {entries.length} opération{entries.length > 1 ? "s" : ""} enregistrée
          {entries.length > 1 ? "s" : ""}
        </p>
        <Button
          variant="outline"
          size="sm"
          className="text-destructive hover:text-destructive"
          onClick={handleClear}
        >
          <Trash2 size={14} />
          Effacer l’historique
        </Button>
      </div>

      <div className="table-scroll">
        <table className="applications-table">
          <thead>
            <tr>
              <th scope="col">Application</th>
              <th scope="col">Format</th>
              <th scope="col">Date</th>
              <th scope="col">Résultat</th>
              <th scope="col">
                <span className="sr-only">Diagnostic</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.id}>
                <td>
                  <div className="flex items-center gap-3">
                    <span className="app-icon">
                      <Package size={18} aria-hidden="true" />
                    </span>
                    <div>
                      <strong>{entry.targetName}</strong>
                      <small className="font-mono text-xs text-muted-foreground block">
                        {entry.nativeId}
                      </small>
                    </div>
                  </div>
                </td>
                <td>
                  <span className="format-badge">
                    {entry.format.toUpperCase()}
                  </span>
                </td>
                <td className="size-cell text-xs">
                  <div className="flex items-center gap-1.5 text-muted-foreground">
                    <Clock size={13} aria-hidden="true" />
                    <span>{formatDate(entry.timestamp)}</span>
                  </div>
                </td>
                <td>
                  {entry.status === "completed" ? (
                    <span className="inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400 font-medium">
                      <CheckCircle2 size={14} /> Réussie
                    </span>
                  ) : (
                    <span className="inline-flex items-center gap-1 text-xs text-destructive font-medium">
                      <AlertTriangle size={14} /> Échouée
                    </span>
                  )}
                </td>
                <td>
                  {entry.diagnostic && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setSelectedEntry(entry)}
                      aria-label="Voir le diagnostic"
                    >
                      <Info size={14} />
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {selectedEntry && (
        <dialog
          className="details-dialog max-w-lg"
          open
          onClose={() => setSelectedEntry(null)}
        >
          <div className="flex items-start justify-between">
            <h3 className="font-semibold text-base">Diagnostic technique</h3>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setSelectedEntry(null)}
            >
              ×
            </Button>
          </div>
          <p className="text-xs text-muted-foreground mt-2">
            Opération : {selectedEntry.operationId}
          </p>
          <pre className="mt-3 rounded-md bg-muted p-3 text-xs font-mono overflow-x-auto whitespace-pre-wrap">
            {selectedEntry.diagnostic ||
              "Aucun détail d’erreur supplémentaire."}
          </pre>
          <div className="mt-4 flex justify-end">
            <Button variant="outline" onClick={() => setSelectedEntry(null)}>
              Fermer
            </Button>
          </div>
        </dialog>
      )}
    </div>
  );
}
