import { useEffect, useState } from "react";
import {
  NO_FILTERS,
  rememberRecent,
  togglePin,
  type HomeFilters,
} from "../domain/homeFilters";
import type { WorkspaceState } from "./useWorkspace";

/** The brandkit mockup's tabs over Home's file table. */
export type HomeTab = "recent" | "starred" | "all";

/**
 * Home's filters, tab, pinned folders, starred files and recently opened
 * files. Filters live above the views, so leaving Home and coming back keeps
 * them. Pins, stars and recent files are this device's preferences for one
 * folder; a pin or a star is not a permission.
 */
export interface HomeState {
  tab: HomeTab;
  setTab: (tab: HomeTab) => void;
  /** Files starred on this device. */
  starIds: string[];
  toggleStar: (id: string) => void;
  filters: HomeFilters;
  setFilters: (filters: HomeFilters) => void;
  pins: string[];
  togglePin: (folder: string) => void;
  /** Files opened in Folio on this device, newest first. */
  recentIds: string[];
}

interface Stored {
  pins: string[];
  recent: string[];
  stars: string[];
}

const EMPTY: Stored = { pins: [], recent: [], stars: [] };

function key(workspace: WorkspaceState): string | null {
  if (workspace.workspace) return `folio.home.${workspace.workspace.id}`;
  return workspace.source === "samples" ? "folio.home.samples" : null;
}

// Storage can be missing or throw; preferences then last only this session.
function load(storageKey: string | null): Stored {
  if (!storageKey) return EMPTY;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(storageKey) ?? "");
    const strings = (value: unknown) =>
      Array.isArray(value)
        ? value.filter((item): item is string => typeof item === "string")
        : [];
    return {
      pins: strings(parsed?.pins),
      recent: strings(parsed?.recent),
      stars: strings(parsed?.stars),
    };
  } catch {
    return EMPTY;
  }
}

function save(storageKey: string | null, stored: Stored) {
  if (!storageKey) return;
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(stored));
  } catch {
    // Not remembered beyond this session.
  }
}

export function useHome(workspace: WorkspaceState): HomeState {
  const storageKey = key(workspace);
  const [filters, setFilters] = useState<HomeFilters>(NO_FILTERS);
  const [tab, setTab] = useState<HomeTab>("recent");
  const [stored, setStored] = useState<Stored>(() => load(storageKey));

  // Another folder: its own preferences, and filters that fit it.
  useEffect(() => {
    setStored(load(storageKey));
    setFilters(NO_FILTERS);
  }, [storageKey]);

  function update(next: Stored) {
    setStored(next);
    save(storageKey, next);
  }

  const selectedId = workspace.selected?.id;
  useEffect(() => {
    if (!selectedId || !storageKey) return;
    setStored((current) => {
      if (current.recent[0] === selectedId) return current;
      const next = {
        ...current,
        recent: rememberRecent(current.recent, selectedId),
      };
      save(storageKey, next);
      return next;
    });
  }, [selectedId, storageKey]);

  return {
    tab,
    setTab,
    starIds: stored.stars,
    toggleStar: (id) =>
      update({
        ...stored,
        stars: stored.stars.includes(id)
          ? stored.stars.filter((item) => item !== id)
          : [...stored.stars, id],
      }),
    filters,
    setFilters,
    pins: stored.pins,
    togglePin: (folder) =>
      update({ ...stored, pins: togglePin(stored.pins, folder) }),
    recentIds: stored.recent,
  };
}
