import { useCallback, useEffect, useRef, useState } from "react";
import type { Inventory } from "@/lib/contracts";
import { errorMessage, getInventory } from "@/lib/desktop";

export function useInventory() {
  const [inventory, setInventory] = useState<Inventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const paths = useRef<string[]>([]);
  const [extraPaths, setExtraPaths] = useState<string[]>([]);

  const refresh = useCallback(async (sources = paths.current) => {
    const current = ++request.current;
    setLoading(true);
    setError(null);
    try {
      const value = await getInventory(sources);
      if (request.current !== current) return false;
      setInventory(value);
      return true;
    } catch (cause) {
      if (request.current === current) setError(errorMessage(cause));
      return false;
    } finally {
      if (request.current === current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    return () => {
      request.current += 1;
    };
  }, [refresh]);

  const addLocation = async (value: string): Promise<string | null> => {
    const path = value.trim();
    if (!path) return "Renseigne un chemin de fichier ou de dossier.";
    if (paths.current.includes(path)) return "Cet emplacement est déjà ajouté.";
    if (paths.current.length >= 16)
      return "Tu peux ajouter jusqu’à 16 emplacements.";
    const next = [...paths.current, path];
    if (!(await refresh(next))) return "L’analyse n’a pas abouti. Réessaie.";
    paths.current = next;
    setExtraPaths(next);
    return null;
  };

  const removeLocation = async (path: string) => {
    const next = paths.current.filter((item) => item !== path);
    if (await refresh(next)) {
      paths.current = next;
      setExtraPaths(next);
    }
  };

  return {
    inventory,
    loading,
    error,
    refresh,
    extraPaths,
    addLocation,
    removeLocation,
  };
}
