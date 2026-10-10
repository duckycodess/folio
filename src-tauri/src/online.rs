//! Optional online generation through Groq (ADR 0018).
//!
//! Off by default. The user's key lives only in the operating system's
//! keychain; `settings.json` keeps whether it is on and which model. When it is
//! on, summaries and answers use Groq; request interpretation, collection names
//! and embeddings never do. Nothing falls back between local and online
//! generation without the user changing the setting.

use std::sync::atomic::AtomicBool;

use folio_core::contracts::{NativeProviderError, ProviderErrorCode};
use folio_core::hosted::{self, ApiKey, GroqProvider};
use folio_core::models::{ModelStore, OnlineGenerationSettings};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{error, ErrorCode, FolioError};
use crate::{model_store, native_error, run_blocking};

/// Keychain entry for the key. The service is the app identifier.
const KEYCHAIN_SERVICE: &str = "dev.folio.desktop";
const KEYCHAIN_ACCOUNT: &str = "groq-api-key";

pub(crate) trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, NativeProviderError>;
    fn set(&self, secret: &str) -> Result<(), NativeProviderError>;
    fn delete(&self) -> Result<(), NativeProviderError>;
}

/// Windows Credential Manager, the macOS Keychain, or the Linux kernel
/// keyring (development only).
struct KeychainStore;

impl KeychainStore {
    fn entry() -> Result<keyring::Entry, NativeProviderError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(keychain_error)
    }
}

impl SecretStore for KeychainStore {
    fn get(&self) -> Result<Option<String>, NativeProviderError> {
        match Self::entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(failure) => Err(keychain_error(failure)),
        }
    }

    fn set(&self, secret: &str) -> Result<(), NativeProviderError> {
        Self::entry()?.set_password(secret).map_err(keychain_error)
    }

    fn delete(&self) -> Result<(), NativeProviderError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(failure) => Err(keychain_error(failure)),
        }
    }
}

fn keychain_error(failure: keyring::Error) -> NativeProviderError {
    NativeProviderError {
        code: ProviderErrorCode::IoError,
        message: "Folio couldn't use this computer's keychain.".into(),
        // keyring's messages describe the store, never the secret.
        detail: Some(failure.to_string()),
    }
}

/// Managed state for the key store. Its own type: Tauri keeps one state per type.
pub(crate) struct OnlineState(Box<dyn SecretStore>);

impl Default for OnlineState {
    fn default() -> Self {
        Self(Box::new(KeychainStore))
    }
}

impl OnlineState {
    pub(crate) fn secrets(&self) -> &dyn SecretStore {
        self.0.as_ref()
    }
}

/// What the UI may know: never the key itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnlineGenerationStatus {
    enabled: bool,
    model_id: String,
    models: Vec<&'static str>,
    key_stored: bool,
}

fn settings(store: &ModelStore) -> Result<OnlineGenerationSettings, NativeProviderError> {
    Ok(store
        .online_generation()
        .map_err(native_error)?
        .unwrap_or(OnlineGenerationSettings {
            enabled: false,
            model_id: hosted::DEFAULT_GROQ_MODEL.into(),
        }))
}

pub(crate) fn status(
    store: &ModelStore,
    secrets: &dyn SecretStore,
) -> Result<OnlineGenerationStatus, NativeProviderError> {
    let settings = settings(store)?;
    Ok(OnlineGenerationStatus {
        enabled: settings.enabled,
        model_id: settings.model_id,
        models: hosted::GROQ_MODELS.to_vec(),
        key_stored: secrets.get()?.is_some(),
    })
}

/// Store a key only after `check` (Groq's model list) accepts it.
pub(crate) fn save_key(
    store: &ModelStore,
    secrets: &dyn SecretStore,
    key: &str,
    check: impl FnOnce(ApiKey) -> Result<(), NativeProviderError>,
) -> Result<OnlineGenerationStatus, FolioError> {
    let api_key = ApiKey::new(key).map_err(|failure| NativeProviderError {
        code: ProviderErrorCode::OnlineKeyRejected,
        message: failure.to_string(),
        detail: None,
    })?;
    let trimmed = api_key.expose().to_owned();
    check(api_key)?;
    secrets.set(&trimmed)?;
    Ok(status(store, secrets)?)
}

/// Forget the key and turn online generation off.
pub(crate) fn forget_key(
    store: &ModelStore,
    secrets: &dyn SecretStore,
) -> Result<OnlineGenerationStatus, FolioError> {
    secrets.delete()?;
    let settings = settings(store)?;
    store
        .set_online_generation(Some(OnlineGenerationSettings {
            enabled: false,
            ..settings
        }))
        .map_err(native_error)?;
    Ok(status(store, secrets)?)
}

/// Turning it on needs the user's consent to send passages to Groq and a stored key.
pub(crate) fn set_enabled(
    store: &ModelStore,
    secrets: &dyn SecretStore,
    enabled: bool,
    model_id: &str,
    consent: bool,
) -> Result<OnlineGenerationStatus, FolioError> {
    if !hosted::is_supported_model(model_id) {
        return Err(error(
            ErrorCode::Internal,
            format!("Folio doesn't support the Groq model {model_id}."),
        )
        .with_detail("reportedCode", "unsupportedModel"));
    }
    if enabled && !consent {
        return Err(error(
            ErrorCode::Internal,
            "Online generation stays off until you agree to send passages to Groq.",
        )
        .with_detail("reportedCode", "consentRequired"));
    }
    if enabled && secrets.get()?.is_none() {
        return Err(missing_key().into());
    }
    store
        .set_online_generation(Some(OnlineGenerationSettings {
            enabled,
            model_id: model_id.into(),
        }))
        .map_err(native_error)?;
    Ok(status(store, secrets)?)
}

fn missing_key() -> NativeProviderError {
    NativeProviderError {
        code: ProviderErrorCode::OnlineKeyMissing,
        message: "Online generation is on, but no Groq key is saved.".into(),
        detail: None,
    }
}

/// The Groq provider when online generation is on, or `None` to use the local
/// model. Building it makes no network request.
pub(crate) fn writer(
    store: &ModelStore,
    secrets: &dyn SecretStore,
) -> Result<Option<GroqProvider>, NativeProviderError> {
    let settings = settings(store)?;
    if !settings.enabled {
        return Ok(None);
    }
    let key = secrets.get()?.ok_or_else(missing_key)?;
    let key = ApiKey::new(&key).map_err(|_| missing_key())?;
    GroqProvider::new(key, &settings.model_id)
        .map(Some)
        .map_err(native_error)
}

#[tauri::command]
pub(crate) async fn online_generation_status(
    app: AppHandle,
    online: State<'_, OnlineState>,
) -> Result<OnlineGenerationStatus, FolioError> {
    let store = model_store(&app)?;
    Ok(status(&store, online.secrets())?)
}

#[tauri::command]
pub(crate) async fn save_online_key(
    app: AppHandle,
    key: String,
) -> Result<OnlineGenerationStatus, FolioError> {
    run_blocking::<_, FolioError, _>(move || {
        use tauri::Manager;
        let store = model_store(&app)?;
        let online = app.state::<OnlineState>();
        save_key(&store, online.secrets(), &key, |api_key| {
            GroqProvider::new(api_key, hosted::DEFAULT_GROQ_MODEL)
                .and_then(|groq| groq.check_key(&AtomicBool::new(false)))
                .map_err(native_error)
        })
    })
    .await
}

#[tauri::command]
pub(crate) async fn forget_online_key(
    app: AppHandle,
    online: State<'_, OnlineState>,
) -> Result<OnlineGenerationStatus, FolioError> {
    let store = model_store(&app)?;
    forget_key(&store, online.secrets())
}

#[tauri::command]
pub(crate) async fn set_online_generation(
    app: AppHandle,
    online: State<'_, OnlineState>,
    enabled: bool,
    model_id: String,
    consent: bool,
) -> Result<OnlineGenerationStatus, FolioError> {
    let store = model_store(&app)?;
    set_enabled(&store, online.secrets(), enabled, &model_id, consent)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use folio_core::generation::GenerationProvider;
    use std::sync::Mutex;

    #[derive(Default)]
    pub(crate) struct MemoryStore(Mutex<Option<String>>);

    impl SecretStore for MemoryStore {
        fn get(&self) -> Result<Option<String>, NativeProviderError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn set(&self, secret: &str) -> Result<(), NativeProviderError> {
            *self.0.lock().unwrap() = Some(secret.into());
            Ok(())
        }

        fn delete(&self) -> Result<(), NativeProviderError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    const KEY: &str = "test-key-not-real-0123456789";

    fn store() -> (tempfile::TempDir, ModelStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(temp.path()).unwrap();
        (temp, store)
    }

    fn accept(_: ApiKey) -> Result<(), NativeProviderError> {
        Ok(())
    }

    #[test]
    fn online_generation_is_off_until_the_user_turns_it_on() {
        let (_temp, store) = store();
        let secrets = MemoryStore::default();
        let status = status(&store, &secrets).unwrap();
        assert!(!status.enabled);
        assert!(!status.key_stored);
        assert_eq!(status.model_id, hosted::DEFAULT_GROQ_MODEL);
        // Off means the local model is used and no provider is built.
        assert!(writer(&store, &secrets).unwrap().is_none());
    }

    #[test]
    fn a_key_is_stored_only_after_groq_accepts_it_and_never_shown() {
        let (temp, store) = store();
        let secrets = MemoryStore::default();
        let refused = save_key(&store, &secrets, KEY, |_| {
            Err(NativeProviderError {
                code: ProviderErrorCode::OnlineKeyRejected,
                message: "Groq refused the saved key.".into(),
                detail: None,
            })
        })
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::ModelLoadFailed);
        assert_eq!(refused.detail("reason"), Some("keyRejected"));
        assert_eq!(secrets.get().unwrap(), None);

        let saved = save_key(&store, &secrets, &format!("  {KEY}\n"), accept).unwrap();
        assert!(saved.key_stored);
        assert_eq!(secrets.get().unwrap().as_deref(), Some(KEY));
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains(KEY));
        assert!(!std::fs::read_to_string(temp.path().join("settings.json"))
            .unwrap_or_default()
            .contains(KEY));
    }

    #[test]
    fn turning_it_on_needs_consent_and_a_key() {
        let (_temp, store) = store();
        let secrets = MemoryStore::default();
        let model = hosted::DEFAULT_GROQ_MODEL;
        let no_key = set_enabled(&store, &secrets, true, model, true).unwrap_err();
        assert_eq!(no_key.code, ErrorCode::ModelNotInstalled);
        assert_eq!(no_key.detail("component"), Some("onlineKey"));

        save_key(&store, &secrets, KEY, accept).unwrap();
        let no_consent = set_enabled(&store, &secrets, true, model, false).unwrap_err();
        assert_eq!(no_consent.detail("reportedCode"), Some("consentRequired"));
        assert!(!status(&store, &secrets).unwrap().enabled);

        let unsupported =
            set_enabled(&store, &secrets, true, "llama-3.1-8b-instant", true).unwrap_err();
        assert_eq!(unsupported.detail("reportedCode"), Some("unsupportedModel"));

        let on = set_enabled(&store, &secrets, true, "openai/gpt-oss-120b", true).unwrap();
        assert!(on.enabled);
        let groq = writer(&store, &secrets).unwrap().unwrap();
        assert_eq!(groq.model_id(), "openai/gpt-oss-120b");
        assert_eq!(groq.origin(), folio_core::contracts::GenerationOrigin::Groq);
    }

    #[test]
    fn forgetting_the_key_turns_online_generation_off() {
        let (_temp, store) = store();
        let secrets = MemoryStore::default();
        save_key(&store, &secrets, KEY, accept).unwrap();
        set_enabled(&store, &secrets, true, hosted::DEFAULT_GROQ_MODEL, true).unwrap();
        let status = forget_key(&store, &secrets).unwrap();
        assert!(!status.enabled);
        assert!(!status.key_stored);
        assert!(writer(&store, &secrets).unwrap().is_none());
    }

    #[test]
    fn a_key_removed_outside_folio_is_reported_not_skipped() {
        let (_temp, store) = store();
        let secrets = MemoryStore::default();
        save_key(&store, &secrets, KEY, accept).unwrap();
        set_enabled(&store, &secrets, true, hosted::DEFAULT_GROQ_MODEL, true).unwrap();
        secrets.delete().unwrap();
        // No silent fallback to the local model.
        let failure = writer(&store, &secrets).unwrap_err();
        assert_eq!(failure.code, ProviderErrorCode::OnlineKeyMissing);
    }
}
