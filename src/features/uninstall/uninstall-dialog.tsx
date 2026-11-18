import { useEffect, useRef, useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  HardDrive,
  LoaderCircle,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import type {
  ApplicationInstallation,
  Operation,
  UninstallPlan,
} from "@/lib/contracts";
import {
  confirmUninstall,
  errorMessage,
  isDesktop,
  prepareUninstall,
} from "@/lib/desktop";
import { formatLabel, scopeLabel } from "@/lib/installations";
import { formatSize } from "@/lib/utils";

interface UninstallDialogProps {
  installation: ApplicationInstallation | null;
  onClose: () => void;
  onSuccess: () => void;
}

type DialogPhase = "preparing" | "preview" | "executing" | "result";

export function UninstallDialog({
  installation,
  onClose,
  onSuccess,
}: UninstallDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [phase, setPhase] = useState<DialogPhase>("preparing");
  const [plan, setPlan] = useState<UninstallPlan | null>(null);
  const [operation, setOperation] = useState<Operation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [purgeConfig, setPurgeConfig] = useState(false);
  const [removeData, setRemoveData] = useState(false);

  useEffect(() => {
    const el = dialog.current;
    if (installation && el && !el.open) {
      el.showModal();
      setPhase("preparing");
      setError(null);
      setSubmitting(false);
      setOperation(null);
      setPurgeConfig(false);
      setRemoveData(false);
      loadPlan(installation, { purgeConfig: false, removeData: false });
    }
    if (!installation && el?.open) {
      el.close();
    }
  }, [installation]);

  const loadPlan = (
    item: ApplicationInstallation,
    options: { purgeConfig: boolean; removeData: boolean },
  ) => {
    if (!isDesktop()) {
      setError("La désinstallation nécessite l’application desktop.");
      setPhase("preview");
      return;
    }
    setPhase("preparing");
    prepareUninstall(item, options)
      .then((generatedPlan) => {
        setPlan(generatedPlan);
        setPhase("preview");
      })
      .catch((err) => {
        setError(errorMessage(err));
        setPhase("preview");
      });
  };

  const handleOptionChange = (newPurge: boolean, newRemoveData: boolean) => {
    setPurgeConfig(newPurge);
    setRemoveData(newRemoveData);
    if (installation) {
      loadPlan(installation, {
        purgeConfig: newPurge,
        removeData: newRemoveData,
      });
    }
  };

  const handleConfirm = () => {
    if (!plan || submitting || plan.isBlocked) return;
    setSubmitting(true);
    setPhase("executing");
    setError(null);

    confirmUninstall(plan.id)
      .then((op) => {
        setOperation(op);
        setPhase("result");
      })
      .catch((err) => {
        setError(errorMessage(err));
        setPhase("result");
      })
      .finally(() => {
        setSubmitting(false);
      });
  };

  const handleClose = () => {
    const wasSuccess = operation?.status === "completed";
    dialog.current?.close();
    onClose();
    if (wasSuccess) {
      onSuccess();
    }
  };

  return (
    <dialog
      ref={dialog}
      className="details-dialog max-w-xl"
      aria-labelledby="uninstall-title"
      onClose={handleClose}
    >
      {installation && (
        <div className="space-y-4">
          <div className="flex items-start justify-between gap-4">
            <div className="flex items-center gap-3">
              <span className="app-icon text-destructive">
                <Trash2 aria-hidden="true" />
              </span>
              <div>
                <h2 id="uninstall-title" className="text-lg font-semibold">
                  Désinstallation de {installation.name}
                </h2>
                <p className="text-xs text-muted-foreground">
                  {formatLabel(installation.format)} ·{" "}
                  {scopeLabel(installation)}
                </p>
              </div>
            </div>
            {phase !== "executing" && (
              <Button
                variant="ghost"
                size="icon"
                aria-label="Fermer la désinstallation"
                onClick={handleClose}
              >
                <X />
              </Button>
            )}
          </div>

          {phase === "preparing" && (
            <div className="empty-state py-8" role="status">
              <LoaderCircle className="animate-spin text-primary" size={32} />
              <p className="text-sm font-medium">
                Analyse des conséquences de la suppression…
              </p>
            </div>
          )}

          {phase === "preview" && (
            <>
              {error ? (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
                  <div className="flex items-start gap-3">
                    <AlertCircle size={18} className="shrink-0 mt-0.5" />
                    <p>{error}</p>
                  </div>
                </div>
              ) : plan?.isBlocked ? (
                <div className="space-y-3">
                  <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-800 dark:text-amber-300">
                    <div className="flex items-start gap-3">
                      <ShieldAlert
                        size={20}
                        className="shrink-0 mt-0.5 text-amber-600 dark:text-amber-400"
                      />
                      <div className="space-y-1">
                        <strong className="font-semibold">
                          Suppression bloquée pour la sécurité du système
                        </strong>
                        <p>
                          {plan.blockedReason ??
                            "Ce composant est essentiel au fonctionnement du système et ne peut pas être retiré."}
                        </p>
                      </div>
                    </div>
                  </div>
                  {plan.debPreview &&
                    plan.debPreview.blockingDependents.length > 0 && (
                      <div className="space-y-1.5">
                        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                          Dépendances bloquantes détectées par APT (
                          {plan.debPreview.blockingDependents.length})
                        </p>
                        <ul className="max-h-32 overflow-y-auto rounded-md border bg-background p-2 text-xs font-mono space-y-1">
                          {plan.debPreview.blockingDependents.map((dep) => (
                            <li key={dep} className="text-foreground break-all">
                              • {dep}
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                </div>
              ) : (
                <div className="space-y-4">
                  <div className="rounded-md bg-muted/60 p-3 text-sm">
                    <p className="font-medium text-foreground">
                      Conséquences de l’opération :
                    </p>
                    <p className="mt-1 text-xs text-muted-foreground">
                      {plan?.dataImpact}
                    </p>
                  </div>

                  {plan?.debPreview ? (
                    <div className="space-y-3">
                      {plan.debPreview.dependentPackages.length > 0 && (
                        <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-300 flex items-start gap-2.5">
                          <AlertTriangle
                            size={16}
                            className="shrink-0 mt-0.5 text-amber-600 dark:text-amber-400"
                            aria-hidden="true"
                          />
                          <span>
                            <strong>
                              {plan.debPreview.dependentPackages.length} autre
                              {plan.debPreview.dependentPackages.length > 1
                                ? "s"
                                : ""}{" "}
                              paquet
                              {plan.debPreview.dependentPackages.length > 1
                                ? "s"
                                : ""}{" "}
                              dépendant
                              {plan.debPreview.dependentPackages.length > 1
                                ? "s"
                                : ""}
                            </strong>{" "}
                            ser
                            {plan.debPreview.dependentPackages.length > 1
                              ? "ont"
                              : "a"}{" "}
                            également retiré
                            {plan.debPreview.dependentPackages.length > 1
                              ? "s"
                              : ""}{" "}
                            par APT. Vérifiez la liste complète ci-dessous avant
                            de confirmer.
                          </span>
                        </div>
                      )}

                      <div className="space-y-1.5">
                        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                          Paquets DEB retirés par APT (
                          {plan.debPreview.removedPackages.length})
                        </p>
                        <ul className="max-h-44 overflow-y-auto rounded-md border bg-background divide-y divide-border/60 text-xs">
                          {plan.debPreview.removedPackages.map((pkg) => (
                            <li
                              key={pkg.name}
                              className="p-2.5 flex items-start justify-between gap-3"
                            >
                              <div className="min-w-0 space-y-0.5">
                                <div className="flex flex-wrap items-center gap-1.5">
                                  <span className="font-mono font-medium text-foreground break-all">
                                    {pkg.name}
                                  </span>
                                  <span className="format-badge">
                                    {pkg.isTarget ? "CIBLE" : "DÉPENDANCE"}
                                  </span>
                                  {pkg.action === "purge" && (
                                    <span className="format-badge">PURGE</span>
                                  )}
                                </div>
                                {pkg.description && (
                                  <p className="text-[11px] text-muted-foreground">
                                    {pkg.description}
                                  </p>
                                )}
                                {pkg.version && (
                                  <p className="text-[10px] font-mono text-muted-foreground">
                                    v{pkg.version}
                                  </p>
                                )}
                              </div>
                              {pkg.installedSize !== null && (
                                <span className="size-cell text-[11px] shrink-0">
                                  {formatSize(pkg.installedSize)}
                                </span>
                              )}
                            </li>
                          ))}
                        </ul>
                      </div>

                      {plan.debPreview.newlyInstalledPackages.length > 0 && (
                        <div className="space-y-1.5">
                          <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                            Paquets de remplacement proposés par APT (
                            {plan.debPreview.newlyInstalledPackages.length})
                          </p>
                          <ul className="max-h-24 overflow-y-auto rounded-md border bg-background p-2 text-xs font-mono space-y-1">
                            {plan.debPreview.newlyInstalledPackages.map(
                              (pkg) => (
                                <li
                                  key={pkg}
                                  className="text-foreground break-all"
                                >
                                  + {pkg}
                                </li>
                              ),
                            )}
                          </ul>
                        </div>
                      )}

                      {plan.debPreview.autoremoveCandidates.length > 0 && (
                        <details className="rounded-md border border-border/80 bg-muted/30 p-2.5 text-xs">
                          <summary className="cursor-pointer font-medium text-muted-foreground">
                            Dépendances automatiques non requises (
                            {plan.debPreview.autoremoveCandidates.length}) —
                            conservées (autoremove désactivé)
                          </summary>
                          <ul className="mt-2 max-h-28 overflow-y-auto font-mono text-[11px] text-muted-foreground space-y-0.5 pl-2">
                            {plan.debPreview.autoremoveCandidates.map((pkg) => (
                              <li key={pkg} className="break-all">
                                • {pkg}
                              </li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </div>
                  ) : (
                    plan &&
                    plan.affectedItems.length > 0 && (
                      <div className="space-y-1.5">
                        <p className="text-xs font-semibold text-muted-foreground uppercase tracking-wider">
                          Éléments concernés ({plan.affectedItems.length})
                        </p>
                        <ul className="max-h-32 overflow-y-auto rounded-md border bg-background p-2 text-xs font-mono space-y-1">
                          {plan.affectedItems.map((item, index) => (
                            <li
                              key={index}
                              className="text-foreground break-all"
                            >
                              • {item}
                            </li>
                          ))}
                        </ul>
                      </div>
                    )
                  )}

                  {plan?.estimatedFreedSpace && (
                    <div className="flex items-center gap-2 text-xs text-muted-foreground">
                      <HardDrive size={15} aria-hidden="true" />
                      <span>
                        Espace libéré estimé :{" "}
                        <strong className="text-foreground">
                          {plan.estimatedFreedSpace}
                        </strong>
                      </span>
                    </div>
                  )}

                  {installation.format === "deb" && (
                    <label className="flex items-center gap-2 text-xs cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={purgeConfig}
                        onChange={(e) =>
                          handleOptionChange(e.target.checked, removeData)
                        }
                      />
                      <span>
                        Purger les fichiers de configuration système (/etc)
                      </span>
                    </label>
                  )}

                  {installation.format === "flatpak" && (
                    <label className="flex items-center gap-2 text-xs cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={removeData}
                        onChange={(e) =>
                          handleOptionChange(purgeConfig, e.target.checked)
                        }
                      />
                      <span>
                        Supprimer les données locales de l’application
                        (~/.var/app)
                      </span>
                    </label>
                  )}

                  {plan?.requiresAuth && (
                    <div className="flex items-center gap-2 rounded-md border border-border/80 bg-muted/40 p-2.5 text-xs text-muted-foreground">
                      <ShieldCheck
                        size={16}
                        className="text-primary shrink-0"
                      />
                      <span>
                        Cette opération modifie le système et requiert des
                        privilèges administrateur.
                      </span>
                    </div>
                  )}
                </div>
              )}

              <div className="mt-6 flex justify-end gap-2">
                <Button variant="outline" onClick={handleClose}>
                  Annuler
                </Button>
                <Button
                  variant="destructive"
                  disabled={Boolean(plan?.isBlocked || error || !plan)}
                  onClick={handleConfirm}
                >
                  <Trash2 size={16} />
                  Confirmer la suppression
                </Button>
              </div>
            </>
          )}

          {phase === "executing" && (
            <div className="empty-state py-8 space-y-3" role="status">
              <LoaderCircle
                className="animate-spin text-destructive"
                size={36}
              />
              <h3 className="text-base font-semibold">
                Désinstallation en cours…
              </h3>
              <p className="text-xs text-muted-foreground max-w-sm">
                {plan?.requiresAuth
                  ? "Une fenêtre d’autorisation administrateur (Polkit) peut s’ouvrir. Veuillez confirmer ou annuler l’authentification."
                  : "Veuillez patienter pendant le traitement et la vérification locale de la suppression."}
              </p>
            </div>
          )}

          {phase === "result" && (
            <div className="space-y-4 py-2">
              {operation?.status === "completed" ? (
                <div className="rounded-md border border-emerald-500/30 bg-emerald-500/10 p-4 text-emerald-800 dark:text-emerald-300">
                  <div className="flex items-start gap-3">
                    <CheckCircle2
                      size={22}
                      className="shrink-0 mt-0.5 text-emerald-600 dark:text-emerald-400"
                    />
                    <div className="space-y-1">
                      <strong className="font-semibold text-sm">
                        Désinstallation réussie
                      </strong>
                      <p className="text-xs">{operation.message}</p>
                    </div>
                  </div>
                </div>
              ) : operation?.status === "cancelled" ? (
                <div className="rounded-md border border-amber-500/30 bg-amber-500/10 p-4 text-amber-800 dark:text-amber-300">
                  <div className="flex items-start gap-3">
                    <ShieldAlert
                      size={22}
                      className="shrink-0 mt-0.5 text-amber-600 dark:text-amber-400"
                    />
                    <div className="space-y-1">
                      <strong className="font-semibold text-sm">
                        Désinstallation annulée
                      </strong>
                      <p className="text-xs">
                        {operation.message ||
                          "Autorisation administrateur annulée. Aucune modification n’a été effectuée."}
                      </p>
                    </div>
                  </div>
                </div>
              ) : (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 p-4 text-destructive">
                  <div className="flex items-start gap-3">
                    <AlertTriangle size={22} className="shrink-0 mt-0.5" />
                    <div className="space-y-1">
                      <strong className="font-semibold text-sm">
                        Échec de la désinstallation
                      </strong>
                      <p className="text-xs">
                        {error ||
                          operation?.error?.message ||
                          "L’opération n’a pas pu aboutir."}
                      </p>
                    </div>
                  </div>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2">
                <Button variant="outline" onClick={handleClose}>
                  {operation?.status === "completed"
                    ? "Fermer et actualiser"
                    : "Fermer"}
                </Button>
              </div>
            </div>
          )}
        </div>
      )}
    </dialog>
  );
}
