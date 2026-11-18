import { useEffect, useRef } from "react";
import { Package, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ApplicationInstallation } from "@/lib/contracts";
import { formatSize } from "@/lib/utils";
import { installationSize, scopeLabel } from "@/lib/installations";

export function InstallationDetails({
  installation,
  onClose,
  onRequestUninstall,
}: {
  installation: ApplicationInstallation | null;
  onClose: () => void;
  onRequestUninstall?: (item: ApplicationInstallation) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const element = dialog.current;
    if (installation && element && !element.open) element.showModal();
    if (!installation && element?.open) element.close();
  }, [installation]);

  return (
    <dialog
      ref={dialog}
      className="details-dialog"
      aria-labelledby="installation-title"
      onClose={onClose}
    >
      {installation && (
        <>
          <div className="flex items-start justify-between gap-4">
            <div className="app-icon">
              <Package aria-hidden="true" />
            </div>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Fermer les détails"
              onClick={() => dialog.current?.close()}
            >
              <X />
            </Button>
          </div>
          <h2 id="installation-title" className="mt-5 text-xl font-semibold">
            {installation.name}
          </h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {installation.description || "Aucune description disponible."}
          </p>
          <dl className="detail-fields">
            <div>
              <dt>Identifiant</dt>
              <dd className="break-all font-mono text-xs">
                {installation.nativeId}
              </dd>
            </div>
            <div>
              <dt>Version</dt>
              <dd className="break-all">
                {installation.version || "Non disponible"}
              </dd>
            </div>
            <div>
              <dt>Format</dt>
              <dd>{installation.format.toUpperCase()}</dd>
            </div>
            <div>
              <dt>Installation</dt>
              <dd>{scopeLabel(installation)}</dd>
            </div>
            <div>
              <dt>Taille installée déclarée</dt>
              <dd>{installationSize(installation, formatSize)}</dd>
            </div>
            {installation.reference && (
              <div>
                <dt>
                  {installation.format === "snap" ? "Révision" : "Référence"}
                </dt>
                <dd className="break-all font-mono text-xs">
                  {installation.reference}
                </dd>
              </div>
            )}
            {installation.path && (
              <div>
                <dt>Chemin du fichier</dt>
                <dd className="break-all font-mono text-xs">
                  {installation.path}
                </dd>
              </div>
            )}
            {installation.isExecutable !== null && (
              <div>
                <dt>Permission d’exécution</dt>
                <dd>{installation.isExecutable ? "Présente" : "Absente"}</dd>
              </div>
            )}
          </dl>
          {installation.unavailableReason ? (
            <p className="rounded-md bg-muted p-3 text-sm text-muted-foreground">
              {installation.unavailableReason}
            </p>
          ) : null}
          <div className="mt-5 flex justify-between gap-2">
            {onRequestUninstall && (
              <Button
                variant="destructive"
                onClick={() => {
                  onRequestUninstall(installation);
                  dialog.current?.close();
                }}
              >
                <Trash2 size={16} />
                Désinstaller
              </Button>
            )}
            <Button variant="outline" onClick={() => dialog.current?.close()}>
              Fermer
            </Button>
          </div>
        </>
      )}
    </dialog>
  );
}
