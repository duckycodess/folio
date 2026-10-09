const STORAGE_KEY = "folio.onboarding.completed";

// Storage can be unavailable or throw; onboarding then shows again next time,
// which is the safe direction.
export function loadOnboardingCompleted(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function saveOnboardingCompleted() {
  try {
    window.localStorage.setItem(STORAGE_KEY, "true");
  } catch {
    // Not remembered beyond this session.
  }
}
