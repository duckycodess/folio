use crate::contracts::{
    ModelDescriptor, ModelFile, ModelInstallState, ModelInstallStatus, ModelRole,
    NativeProviderError, ProviderErrorCode,
};
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tar::Archive;
use zip::ZipArchive;

const MANIFEST_JSON: &str = include_str!("../../../resources/model-manifest.json");
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct VerificationKey {
    path: PathBuf,
    expected_bytes: u64,
    expected_sha256: String,
    actual_bytes: u64,
    modified_nanos: u128,
}

static VERIFIED_FILES: OnceLock<Mutex<HashMap<VerificationKey, bool>>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    pub platform: String,
    pub files: Vec<ModelFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelManifest {
    pub schema_version: u32,
    pub models: Vec<ModelDescriptor>,
    pub runtimes: Vec<RuntimeDescriptor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    embedding_model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation_model_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub item_id: String,
    pub file: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
}

pub struct ModelStore {
    data_dir: PathBuf,
    manifest: ModelManifest,
}

impl ModelStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> CoreResult<Self> {
        let manifest: ModelManifest = serde_json::from_str(MANIFEST_JSON)?;
        Self::with_manifest(data_dir, manifest)
    }

    pub fn with_manifest(
        data_dir: impl Into<PathBuf>,
        manifest: ModelManifest,
    ) -> CoreResult<Self> {
        if manifest.schema_version != 1 {
            return Err(CoreError::Message(
                "Unsupported model manifest schema.".into(),
            ));
        }
        for descriptor in &manifest.models {
            validate_id(&descriptor.id)?;
            for file in &descriptor.files {
                validate_relative_path(&file.path)?;
                validate_sha256(&file.sha256)?;
            }
        }
        for descriptor in &manifest.runtimes {
            validate_id(&descriptor.id)?;
            for file in &descriptor.files {
                validate_relative_path(&file.path)?;
                validate_sha256(&file.sha256)?;
            }
        }
        Ok(Self {
            data_dir: data_dir.into(),
            manifest,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    pub fn model(&self, id: &str) -> CoreResult<&ModelDescriptor> {
        self.manifest
            .models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| CoreError::Message(format!("Unknown model: {id}")))
    }

    pub fn runtime(&self, id: &str) -> CoreResult<&RuntimeDescriptor> {
        self.manifest
            .runtimes
            .iter()
            .find(|runtime| runtime.id == id)
            .ok_or_else(|| CoreError::Message(format!("Unknown runtime: {id}")))
    }

    pub fn model_state(&self, id: &str) -> CoreResult<ModelInstallState> {
        self.model_state_with_cache(id, true)
    }

    fn model_state_with_cache(&self, id: &str, use_cache: bool) -> CoreResult<ModelInstallState> {
        let descriptor = self.model(id)?;
        let root = self.model_root(id)?;
        let mut present = 0;
        let mut corrupt = false;
        let mut total_bytes = 0;
        for file in &descriptor.files {
            let path = safe_join(&root, &file.path)?;
            if !path.exists() {
                continue;
            }
            if is_symlink_or_inside_symlink(&root, &path)? {
                corrupt = true;
                continue;
            }
            if !use_cache {
                invalidate_verification_cache(&path);
            }
            let verification = if use_cache {
                verify_file_cached(&path, file)
            } else {
                verify_file(&path, file)
            };
            match verification {
                Ok(()) => {
                    present += 1;
                    total_bytes += file.bytes;
                }
                Err(_) => corrupt = true,
            }
        }
        let status = if present == descriptor.files.len() {
            ModelInstallStatus::Installed
        } else if present == 0 && !corrupt {
            ModelInstallStatus::NotInstalled
        } else {
            ModelInstallStatus::Corrupt
        };
        let error = (status == ModelInstallStatus::Corrupt).then(|| NativeProviderError {
            code: ProviderErrorCode::ModelCorrupt,
            message: "One or more installed model files failed verification.".into(),
            detail: Some(id.into()),
        });
        Ok(ModelInstallState {
            id: id.into(),
            status,
            model_file_bytes: (total_bytes > 0).then_some(total_bytes),
            error,
        })
    }

    pub fn verify_model(&self, id: &str) -> CoreResult<ModelInstallState> {
        self.model_state_with_cache(id, false)
    }

    pub fn verified_model_file(&self, id: &str) -> CoreResult<VerifiedModelFile> {
        let descriptor = self.model(id)?.clone();
        if descriptor.role != ModelRole::Generation {
            return Err(CoreError::Message(format!(
                "Model {id} is not a generation model."
            )));
        }
        let state = self.model_state(id)?;
        if state.status != ModelInstallStatus::Installed {
            return Err(provider(
                ProviderErrorCode::ModelNotInstalled,
                "The generation model is not fully verified and installed.",
            ));
        }
        let file = descriptor
            .files
            .iter()
            .find(|file| file.path.ends_with(".gguf"))
            .ok_or_else(|| CoreError::Message("Generation model has no GGUF file.".into()))?;
        let root = self.model_root(id)?;
        let path = safe_join(&root, &file.path)?;
        if is_symlink_or_inside_symlink(&root, &path)? || !path.is_file() {
            return Err(provider(
                ProviderErrorCode::ModelCorrupt,
                "The verified generation model path is unavailable.",
            ));
        }
        Ok(VerifiedModelFile { descriptor, path })
    }

    pub fn verified_file_path(&self, id: &str, relative_path: &str) -> CoreResult<PathBuf> {
        let descriptor = self.model(id)?.clone();
        let state = self.model_state(id)?;
        if state.status != ModelInstallStatus::Installed {
            return Err(provider(
                ProviderErrorCode::ModelNotInstalled,
                "The model is not fully verified and installed.",
            ));
        }
        if !descriptor
            .files
            .iter()
            .any(|file| file.path == relative_path)
        {
            return Err(CoreError::Message(format!(
                "Model {id} has no file named {relative_path}."
            )));
        }
        let root = self.model_root(id)?;
        let path = safe_join(&root, relative_path)?;
        if is_symlink_or_inside_symlink(&root, &path)? || !path.is_file() {
            return Err(provider(
                ProviderErrorCode::ModelCorrupt,
                "The verified model file path is unavailable.",
            ));
        }
        Ok(path)
    }

    pub fn install_model<F>(
        &self,
        id: &str,
        cancel: &AtomicBool,
        mut on_progress: F,
    ) -> CoreResult<ModelInstallState>
    where
        F: FnMut(DownloadProgress),
    {
        let descriptor = self.model(id)?.clone();
        let root = self.model_root(id)?;
        fs::create_dir_all(&root)?;
        let client = download_client()?;
        for file in descriptor.files {
            if cancel.load(Ordering::Relaxed) {
                return Err(provider(
                    ProviderErrorCode::Cancelled,
                    "Model installation cancelled.",
                ));
            }
            let url = file.download_url.clone().ok_or_else(|| {
                CoreError::Message(format!("Manifest has no download URL for {}.", file.path))
            })?;
            let destination = safe_join(&root, &file.path)?;
            if is_symlink_or_inside_symlink(&root, &destination)? {
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    "Refusing to write through a model-directory symlink.",
                ));
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            let partial = partial_path(&destination);
            if is_symlink_or_inside_symlink(&root, &partial)? {
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    "Refusing to write through a model partial-file symlink.",
                ));
            }
            download_verified(
                &client,
                &url,
                &partial,
                &file,
                cancel,
                "Model installation cancelled.",
                |received| {
                    on_progress(DownloadProgress {
                        item_id: id.into(),
                        file: file.path.clone(),
                        received_bytes: received,
                        total_bytes: file.bytes,
                    })
                },
            )?;
            fs::rename(partial, destination)?;
        }
        let state = self.model_state(id)?;
        if state.status != ModelInstallStatus::Installed {
            return Err(provider(
                ProviderErrorCode::ModelCorrupt,
                "Model did not reach a verified installed state.",
            ));
        }
        Ok(state)
    }

    pub fn remove_model(&self, id: &str) -> CoreResult<()> {
        let root = self.model_root(id)?;
        if !root.exists() {
            return Ok(());
        }
        let models_root = self.models_root();
        fs::create_dir_all(&models_root)?;
        let canonical_root = root
            .canonicalize()
            .map_err(|_| CoreError::Message("The model directory is unavailable.".into()))?;
        let canonical_models = models_root.canonicalize()?;
        if canonical_root.parent() != Some(canonical_models.as_path()) {
            return Err(CoreError::Message(
                "Model removal escaped the model store.".into(),
            ));
        }
        if fs::symlink_metadata(&root)?.file_type().is_symlink() {
            return Err(CoreError::Message(
                "Refusing to remove a model symlink.".into(),
            ));
        }
        fs::remove_dir_all(root)?;
        self.clear_selection(id)
    }

    /// A removed model can no longer be selected for either role.
    fn clear_selection(&self, id: &str) -> CoreResult<()> {
        let mut settings = self.read_settings()?;
        let before = (
            settings.embedding_model_id.clone(),
            settings.generation_model_id.clone(),
        );
        if settings.embedding_model_id.as_deref() == Some(id) {
            settings.embedding_model_id = None;
        }
        if settings.generation_model_id.as_deref() == Some(id) {
            settings.generation_model_id = None;
        }
        if before
            == (
                settings.embedding_model_id.clone(),
                settings.generation_model_id.clone(),
            )
        {
            return Ok(());
        }
        self.write_settings(&settings)
    }

    pub fn select_model(&self, role: ModelRole, id: &str) -> CoreResult<()> {
        let descriptor = self.model(id)?;
        if descriptor.role != role {
            return Err(CoreError::Message(format!(
                "Model {id} has the wrong role."
            )));
        }
        let state = self.model_state(id)?;
        if state.status != ModelInstallStatus::Installed {
            return Err(provider(
                ProviderErrorCode::ModelNotInstalled,
                "Only a fully verified installed model can be selected.",
            ));
        }
        let mut settings = self.read_settings()?;
        match role {
            ModelRole::Embedding => settings.embedding_model_id = Some(id.into()),
            ModelRole::Generation => settings.generation_model_id = Some(id.into()),
        }
        self.write_settings(&settings)
    }

    fn read_settings(&self) -> CoreResult<ModelSettings> {
        let settings_path = self.data_dir.join("settings.json");
        Ok(if settings_path.exists() {
            serde_json::from_str::<ModelSettings>(&fs::read_to_string(&settings_path)?)?
        } else {
            ModelSettings {
                embedding_model_id: None,
                generation_model_id: None,
            }
        })
    }

    fn write_settings(&self, settings: &ModelSettings) -> CoreResult<()> {
        fs::create_dir_all(&self.data_dir)?;
        let settings_path = self.data_dir.join("settings.json");
        let temporary = settings_path.with_extension("json.partial");
        let mut file = File::create(&temporary)?;
        serde_json::to_writer_pretty(&mut file, settings)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(temporary, settings_path)?;
        Ok(())
    }

    pub fn selected_model(&self, role: ModelRole) -> CoreResult<Option<String>> {
        let path = self.data_dir.join("settings.json");
        if !path.exists() {
            return Ok(None);
        }
        let settings: ModelSettings = serde_json::from_str(&fs::read_to_string(path)?)?;
        Ok(match role {
            ModelRole::Embedding => settings.embedding_model_id,
            ModelRole::Generation => settings.generation_model_id,
        })
    }

    /// A runtime is installed only when its install record exists and names an
    /// executable present in the runtime directory. A half-extracted tree has
    /// no record, because the record is written last in the staging copy.
    pub fn runtime_status(&self, id: &str) -> CoreResult<RuntimeStatus> {
        let runtime = self.runtime(id)?;
        let root = self.runtime_root(id)?;
        let executable_path =
            read_install_record(&root).and_then(|record| recorded_executable(&root, &record).ok());
        Ok(RuntimeStatus {
            id: id.into(),
            version: runtime.version.clone(),
            installed: executable_path.is_some(),
            executable_path,
        })
    }

    /// The runtime executable, verified against the SHA-256 recorded when it
    /// was extracted from the hash-pinned archive. Call before every launch.
    pub fn verified_runtime_executable(&self, id: &str) -> CoreResult<PathBuf> {
        self.runtime(id)?;
        let root = self.runtime_root(id)?;
        let record = read_install_record(&root).ok_or_else(|| {
            provider(
                ProviderErrorCode::RuntimeMissing,
                "Install the pinned llama.cpp runtime for this platform first.",
            )
        })?;
        let executable = recorded_executable(&root, &record)?;
        let entry = record
            .files
            .iter()
            .find(|file| file.path == record.executable)
            .ok_or_else(|| {
                provider(
                    ProviderErrorCode::ModelCorrupt,
                    "The runtime install record does not cover its executable.",
                )
            })?;
        verify_file_cached(&executable, entry)?;
        Ok(executable)
    }

    /// Download the pinned archive and extract it into a staging directory,
    /// write the install record last, then swap the staged tree into place.
    /// A failure at any point leaves the previous runtime untouched.
    pub fn install_runtime<F>(
        &self,
        id: &str,
        cancel: &AtomicBool,
        mut on_progress: F,
    ) -> CoreResult<RuntimeStatus>
    where
        F: FnMut(DownloadProgress),
    {
        let descriptor = self.runtime(id)?.clone();
        let root = self.runtime_root(id)?;
        let parent = root
            .parent()
            .ok_or_else(|| CoreError::Message("The runtime directory has no parent.".into()))?
            .to_path_buf();
        fs::create_dir_all(&parent)?;
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let staging = parent.join(format!(".staging-{id}-{unique}"));
        fs::create_dir(&staging)?;
        let tree = staging.join("tree");
        let client = download_client()?;
        let mut stage = || -> CoreResult<()> {
            fs::create_dir(&tree)?;
            let mut archives = Vec::new();
            for file in &descriptor.files {
                let url = file.download_url.clone().ok_or_else(|| {
                    CoreError::Message(format!("Manifest has no download URL for {}.", file.path))
                })?;
                let archive = safe_join(&staging, &file.path)?;
                let partial = partial_path(&archive);
                download_verified(
                    &client,
                    &url,
                    &partial,
                    file,
                    cancel,
                    "Runtime installation cancelled.",
                    |received| {
                        on_progress(DownloadProgress {
                            item_id: id.into(),
                            file: file.path.clone(),
                            received_bytes: received,
                            total_bytes: file.bytes,
                        })
                    },
                )?;
                fs::rename(&partial, &archive)?;
                extract_runtime_archive(&archive, &tree)?;
                archives.push(file.sha256.clone());
            }
            write_install_record(id, &tree, archives)
        };
        if let Err(error) = stage() {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let previous = parent.join(format!(".previous-{id}-{unique}"));
        if root.exists() {
            if let Err(error) = fs::rename(&root, &previous) {
                let _ = fs::remove_dir_all(&staging);
                return Err(error.into());
            }
        }
        if let Err(error) = fs::rename(&tree, &root) {
            if previous.exists() {
                let _ = fs::rename(&previous, &root);
            }
            let _ = fs::remove_dir_all(&staging);
            return Err(error.into());
        }
        let _ = fs::remove_dir_all(&previous);
        let _ = fs::remove_dir_all(&staging);
        self.runtime_status(id)
    }

    fn models_root(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    fn model_root(&self, id: &str) -> CoreResult<PathBuf> {
        validate_id(id)?;
        Ok(self.models_root().join(id))
    }

    fn runtime_root(&self, id: &str) -> CoreResult<PathBuf> {
        validate_id(id)?;
        Ok(self.data_dir.join("runtime").join("llama.cpp").join(id))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub id: String,
    pub version: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct VerifiedModelFile {
    pub descriptor: ModelDescriptor,
    pub path: PathBuf,
}

/// HTTPS-only client for pinned public downloads; integrity comes from the
/// manifest size and SHA-256, never from the host.
fn download_client() -> CoreResult<Client> {
    Ok(Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()?)
}

/// Stream `url` into `partial`, never accepting more bytes than the manifest
/// pins, and verify size and SHA-256. On any error, including cancellation
/// and I/O failures, the partial file is removed.
fn download_verified<F>(
    client: &Client,
    url: &str,
    partial: &Path,
    expected: &ModelFile,
    cancel: &AtomicBool,
    cancelled_message: &str,
    mut on_received: F,
) -> CoreResult<()>
where
    F: FnMut(u64),
{
    let mut attempt = || -> CoreResult<()> {
        let mut response = client.get(url).send()?.error_for_status()?;
        if let Some(length) = response.content_length() {
            if length != expected.bytes {
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    format!(
                        "The server reported {length} bytes for {}; the manifest pins {}.",
                        expected.path, expected.bytes
                    ),
                ));
            }
        }
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(partial)?;
        let mut hasher = Sha256::new();
        let mut received = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(provider(ProviderErrorCode::Cancelled, cancelled_message));
            }
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            received += count as u64;
            if received > expected.bytes {
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    format!("{} is larger than the manifest allows.", expected.path),
                ));
            }
            output.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
            on_received(received);
        }
        output.flush()?;
        output.sync_all()?;
        if received != expected.bytes || hex::encode(hasher.finalize()) != expected.sha256 {
            return Err(provider(
                ProviderErrorCode::ModelCorrupt,
                format!(
                    "Downloaded {} failed size or SHA-256 verification.",
                    expected.path
                ),
            ));
        }
        Ok(())
    };
    let result = attempt();
    if result.is_err() {
        let _ = fs::remove_file(partial);
    }
    result
}

fn provider(code: ProviderErrorCode, message: impl Into<String>) -> CoreError {
    CoreError::Provider(NativeProviderErrorError::new(code, message))
}

fn validate_id(id: &str) -> CoreResult<()> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(CoreError::Message(
            "Model and runtime IDs must be simple names.".into(),
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> CoreResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CoreError::Message(
            "Manifest SHA-256 values must be 64 hex characters.".into(),
        ));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> CoreResult<()> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() {
        return Err(CoreError::Message(
            "Manifest paths must be relative.".into(),
        ));
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(CoreError::Message(
                "Manifest paths cannot escape their store.".into(),
            ));
        }
    }
    Ok(())
}

fn safe_join(root: &Path, relative: &str) -> CoreResult<PathBuf> {
    validate_relative_path(relative)?;
    Ok(root.join(relative))
}

fn partial_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.partial", path.display()))
}

fn is_symlink_or_inside_symlink(root: &Path, path: &Path) -> CoreResult<bool> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CoreError::Message("Path escaped its store.".into()))?;
    let mut current = root.to_path_buf();
    if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
        return Ok(true);
    }
    for component in relative.components() {
        if let Component::Normal(name) = component {
            current.push(name);
            if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn verify_file(path: &Path, descriptor: &ModelFile) -> CoreResult<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != descriptor.bytes {
        return Err(provider(
            ProviderErrorCode::ModelCorrupt,
            format!("{} has an unexpected size.", descriptor.path),
        ));
    }
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if hex::encode(hasher.finalize()) != descriptor.sha256 {
        return Err(provider(
            ProviderErrorCode::ModelCorrupt,
            format!("{} has an unexpected SHA-256.", descriptor.path),
        ));
    }
    Ok(())
}

fn verify_file_cached(path: &Path, descriptor: &ModelFile) -> CoreResult<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() != descriptor.bytes {
        return verify_file(path, descriptor);
    }
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |value| value.as_nanos());
    let key = VerificationKey {
        path: path.to_path_buf(),
        expected_bytes: descriptor.bytes,
        expected_sha256: descriptor.sha256.clone(),
        actual_bytes: metadata.len(),
        modified_nanos,
    };
    let cache = VERIFIED_FILES.get_or_init(|| Mutex::new(HashMap::new()));
    if cache
        .lock()
        .ok()
        .and_then(|entries| entries.get(&key).copied())
        .is_some_and(|verified| verified)
    {
        return Ok(());
    }
    let result = verify_file(path, descriptor);
    if result.is_ok() {
        if let Ok(mut entries) = cache.lock() {
            entries.insert(key, true);
        }
    }
    result
}

fn invalidate_verification_cache(path: &Path) {
    let Some(cache) = VERIFIED_FILES.get() else {
        return;
    };
    if let Ok(mut entries) = cache.lock() {
        entries.retain(|key, _| key.path != path);
    }
}

/// Written last into a staged runtime tree: the archive hashes it came from,
/// the executable, and the SHA-256 of every extracted regular file.
const RUNTIME_INSTALL_RECORD: &str = "folio-runtime-install.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInstallRecord {
    schema_version: u32,
    runtime_id: String,
    archive_sha256: Vec<String>,
    executable: String,
    files: Vec<ModelFile>,
}

fn write_install_record(id: &str, tree: &Path, archive_sha256: Vec<String>) -> CoreResult<()> {
    let executable = find_runtime_executable(tree).ok_or_else(|| {
        CoreError::Archive("The runtime archive has no llama-server executable.".into())
    })?;
    let mut files = Vec::new();
    collect_regular_files(tree, tree, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let record = RuntimeInstallRecord {
        schema_version: 1,
        runtime_id: id.into(),
        archive_sha256,
        executable: relative_slash_path(tree, &executable)?,
        files,
    };
    let path = tree.join(RUNTIME_INSTALL_RECORD);
    let mut output = File::create(&path)?;
    output.write_all(&serde_json::to_vec_pretty(&record)?)?;
    output.sync_all()?;
    Ok(())
}

fn read_install_record(root: &Path) -> Option<RuntimeInstallRecord> {
    let path = root.join(RUNTIME_INSTALL_RECORD);
    if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn recorded_executable(root: &Path, record: &RuntimeInstallRecord) -> CoreResult<PathBuf> {
    let executable = safe_join(root, &record.executable)?;
    if is_symlink_or_inside_symlink(root, &executable)? || !is_executable_file(&executable) {
        return Err(provider(
            ProviderErrorCode::RuntimeMissing,
            "The installed llama.cpp runtime has no usable executable.",
        ));
    }
    Ok(executable)
}

fn collect_regular_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<ModelFile>,
) -> CoreResult<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_regular_files(root, &path, files)?;
        } else if metadata.is_file() {
            files.push(ModelFile {
                path: relative_slash_path(root, &path)?,
                sha256: file_sha256(&path)?,
                bytes: metadata.len(),
                download_url: None,
            });
        }
    }
    Ok(())
}

fn relative_slash_path(root: &Path, path: &Path) -> CoreResult<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CoreError::Message("Path escaped its store.".into()))?;
    Ok(relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

fn file_sha256(path: &Path) -> CoreResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn find_runtime_executable(root: &Path) -> Option<PathBuf> {
    if !root.is_dir() || fs::symlink_metadata(root).ok()?.file_type().is_symlink() {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            continue;
        }
        if path.is_dir() {
            if let Some(found) = find_runtime_executable(&path) {
                return Some(found);
            }
        } else if matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("llama-server") | Some("llama-server.exe")
        ) && is_executable_file(&path)
        {
            return Some(path);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Extract a hash-verified runtime archive. Regular files and directories are
/// written first; symbolic links are created afterwards and only when their
/// target is a bare sibling filename (the versioned shared-library links the
/// pinned macOS and Linux archives ship). Hard links and other entry types
/// are refused.
fn extract_runtime_archive(archive: &Path, destination: &Path) -> CoreResult<()> {
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut links: Vec<(PathBuf, String)> = Vec::new();
    if name.ends_with(".zip") {
        let file = File::open(archive)?;
        let mut zip =
            ZipArchive::new(file).map_err(|error| CoreError::Archive(error.to_string()))?;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|error| CoreError::Archive(error.to_string()))?;
            let enclosed = entry
                .enclosed_name()
                .ok_or_else(|| CoreError::Archive("Runtime archive path escaped.".into()))?
                .to_path_buf();
            let target = safe_join(destination, &enclosed.to_string_lossy())?;
            if is_symlink_or_inside_symlink(destination, &target)? {
                return Err(CoreError::Archive(
                    "Runtime archive would write through a symlink.".into(),
                ));
            }
            if entry.is_dir() {
                fs::create_dir_all(target)?;
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                let mut link_target = String::new();
                entry.read_to_string(&mut link_target)?;
                links.push((target, link_target));
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = File::create(&target)?;
            std::io::copy(&mut entry, &mut output)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o755))?;
            }
        }
        return create_sibling_links(links);
    }
    if !name.ends_with(".tar.gz") {
        return Err(CoreError::Archive(
            "Unsupported runtime archive format.".into(),
        ));
    }
    let file = File::open(archive)?;
    let decoder = GzDecoder::new(file);
    let mut tar = Archive::new(decoder);
    for entry in tar
        .entries()
        .map_err(|error| CoreError::Archive(error.to_string()))?
    {
        let mut entry = entry.map_err(|error| CoreError::Archive(error.to_string()))?;
        let entry_type = entry.header().entry_type();
        let mode = entry
            .header()
            .mode()
            .map_err(|error| CoreError::Archive(error.to_string()))?;
        if entry_type.is_hard_link() {
            return Err(CoreError::Archive(
                "Runtime archive contains a hard link.".into(),
            ));
        }
        let relative = entry
            .path()
            .map_err(|error| CoreError::Archive(error.to_string()))?
            .to_path_buf();
        let target = safe_join(destination, &relative.to_string_lossy())?;
        if is_symlink_or_inside_symlink(destination, &target)? {
            return Err(CoreError::Archive(
                "Runtime archive would write through a symlink.".into(),
            ));
        }
        if entry_type.is_symlink() {
            let link_target = entry
                .link_name()
                .map_err(|error| CoreError::Archive(error.to_string()))?
                .ok_or_else(|| CoreError::Archive("Runtime archive link has no target.".into()))?
                .to_string_lossy()
                .into_owned();
            links.push((target, link_target));
            continue;
        }
        if entry_type.is_dir() {
            fs::create_dir_all(target)?;
            continue;
        }
        if !entry_type.is_file() {
            return Err(CoreError::Archive(
                "Runtime archive contains an unsupported entry.".into(),
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        std::io::copy(&mut entry, &mut output)?;
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o755))?;
        #[cfg(not(unix))]
        let _ = mode;
    }
    create_sibling_links(links)
}

/// A link target that names a file in the link's own directory.
fn is_sibling_name(target: &str) -> bool {
    !target.is_empty()
        && target != "."
        && target != ".."
        && !target
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':' | '\0'))
}

/// Create the archive's links after every regular file exists. A link may
/// point at another link in the same directory (a version chain); each one
/// must finally resolve to a regular file there.
fn create_sibling_links(mut links: Vec<(PathBuf, String)>) -> CoreResult<()> {
    if links.iter().any(|(_, target)| !is_sibling_name(target)) {
        return Err(CoreError::Archive(
            "Runtime archive link points outside its directory.".into(),
        ));
    }
    while !links.is_empty() {
        let pending = links.len();
        let mut remaining = Vec::new();
        for (link, target) in links {
            if fs::symlink_metadata(&link).is_ok() {
                return Err(CoreError::Archive(
                    "Runtime archive link collides with another entry.".into(),
                ));
            }
            let directory = link.parent().ok_or_else(|| {
                CoreError::Archive("Runtime archive link has no directory.".into())
            })?;
            if fs::metadata(directory.join(&target)).is_ok_and(|metadata| metadata.is_file()) {
                make_symlink(&target, &link)?;
            } else {
                remaining.push((link, target));
            }
        }
        if remaining.len() == pending {
            return Err(CoreError::Archive(
                "Runtime archive link does not resolve to a file in its directory.".into(),
            ));
        }
        links = remaining;
    }
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &str, link: &Path) -> CoreResult<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_symlink(_target: &str, _link: &Path) -> CoreResult<()> {
    Err(CoreError::Archive(
        "Runtime archive links are not supported on this platform.".into(),
    ))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use flate2::write::GzEncoder;
    #[cfg(unix)]
    use flate2::Compression;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use tar::{Builder, Header};

    fn descriptor(bytes: &[u8]) -> ModelDescriptor {
        ModelDescriptor {
            id: "test-model".into(),
            role: ModelRole::Embedding,
            repo: "local/test".into(),
            revision: "rev".into(),
            files: vec![ModelFile {
                path: "nested/model.bin".into(),
                sha256: sha256_bytes(bytes),
                bytes: bytes.len() as u64,
                download_url: None,
            }],
            quantization: "test".into(),
            license: "test".into(),
            runtime: "test".into(),
            optional_pack: false,
        }
    }

    fn store(root: &Path, model: ModelDescriptor) -> ModelStore {
        ModelStore::with_manifest(
            root,
            ModelManifest {
                schema_version: 1,
                models: vec![model],
                runtimes: vec![],
            },
        )
        .unwrap()
    }

    #[test]
    fn failed_downloads_remove_their_partial_file_and_refuse_plain_http() {
        let temp = tempfile::tempdir().unwrap();
        let partial = temp.path().join("model.bin.partial");
        fs::write(&partial, b"stale bytes from an earlier attempt").unwrap();
        let expected = ModelFile {
            path: "model.bin".into(),
            sha256: sha256_bytes(b"x"),
            bytes: 1,
            download_url: None,
        };
        let client = download_client().unwrap();
        // Plain HTTP is refused by the client before any byte is accepted.
        assert!(download_verified(
            &client,
            "http://127.0.0.1:9/model.bin",
            &partial,
            &expected,
            &AtomicBool::new(false),
            "cancelled",
            |_| {},
        )
        .is_err());
        assert!(!partial.exists());
    }

    #[test]
    fn verifies_size_and_hash_before_marking_installed() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"verified-model";
        let store = store(temp.path(), descriptor(bytes));
        let file = temp.path().join("models/test-model/nested/model.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, bytes).unwrap();
        assert_eq!(
            store.verify_model("test-model").unwrap().status,
            ModelInstallStatus::Installed
        );
        fs::write(&file, b"tampered").unwrap();
        assert_eq!(
            store.verify_model("test-model").unwrap().status,
            ModelInstallStatus::Corrupt
        );
    }

    #[test]
    fn explicit_verification_invalidates_cached_file_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"cached-model";
        let store = store(temp.path(), descriptor(bytes));
        let file = temp.path().join("models/test-model/nested/model.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, bytes).unwrap();

        assert_eq!(
            store.model_state("test-model").unwrap().status,
            ModelInstallStatus::Installed
        );
        assert!(VERIFIED_FILES
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.path == file));
        assert_eq!(
            store.verify_model("test-model").unwrap().status,
            ModelInstallStatus::Installed
        );
        assert!(!VERIFIED_FILES
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .keys()
            .any(|key| key.path == file));
    }

    #[test]
    #[cfg(unix)]
    fn rejects_manifest_escape_and_symlink_writes() {
        assert!(validate_relative_path("../model.bin").is_err());
        assert!(validate_relative_path("/tmp/model.bin").is_err());
        let temp = tempfile::tempdir().unwrap();
        let store = store(temp.path(), descriptor(b"model"));
        let outside = temp.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        let model_root = temp.path().join("models/test-model");
        fs::create_dir_all(model_root.parent().unwrap()).unwrap();
        symlink(&outside, &model_root).unwrap();
        assert!(store.verify_model("test-model").is_ok());
        assert!(store.remove_model("test-model").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn rejects_existing_partial_symlink_before_a_download() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models/test-model");
        let destination = root.join("nested/model.bin");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, partial_path(&destination)).unwrap();

        assert!(is_symlink_or_inside_symlink(&root, &partial_path(&destination)).unwrap());
    }

    #[test]
    fn dot_ids_are_not_simple_names() {
        for id in [".", "..", "", "a/b", "a\\b"] {
            assert!(validate_id(id).is_err(), "{id:?} was accepted");
        }
        assert!(validate_id("qwen3-0.6b-q4-k-m").is_ok());
    }

    #[test]
    fn removing_a_selected_model_clears_its_selection() {
        let temp = tempfile::tempdir().unwrap();
        let bytes = b"model-bytes";
        let store = store(temp.path(), descriptor(bytes));
        let path = temp.path().join("models/test-model/nested/model.bin");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        store
            .select_model(ModelRole::Embedding, "test-model")
            .unwrap();
        assert_eq!(
            store
                .selected_model(ModelRole::Embedding)
                .unwrap()
                .as_deref(),
            Some("test-model")
        );
        store.remove_model("test-model").unwrap();
        assert!(store
            .selected_model(ModelRole::Embedding)
            .unwrap()
            .is_none());
        assert!(!path.exists());
    }

    #[test]
    fn removing_a_model_does_not_touch_model_lab() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(temp.path(), descriptor(b"model"));
        let model_file = temp.path().join("models/test-model/nested/model.bin");
        fs::create_dir_all(model_file.parent().unwrap()).unwrap();
        fs::write(model_file, b"model").unwrap();
        let lab_file = temp.path().join("model-lab/results-v1.jsonl");
        fs::create_dir_all(lab_file.parent().unwrap()).unwrap();
        fs::write(&lab_file, "record\n").unwrap();
        store.remove_model("test-model").unwrap();
        assert!(!temp.path().join("models/test-model").exists());
        assert_eq!(fs::read_to_string(lab_file).unwrap(), "record\n");
    }

    #[cfg(unix)]
    #[test]
    fn tar_runtime_extraction_preserves_executable_mode() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("runtime.tar.gz");
        let archive = File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(archive, Compression::default());
        let mut builder = Builder::new(encoder);
        let bytes = b"#!/bin/sh\n";
        let mut header = Header::new_gnu();
        header.set_path("llama-b11524/llama-server").unwrap();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &bytes[..]).unwrap();
        let encoder = builder.into_inner().unwrap();
        encoder.finish().unwrap();

        let destination = temp.path().join("runtime");
        fs::create_dir_all(&destination).unwrap();
        extract_runtime_archive(&archive_path, &destination).unwrap();
        let executable = destination.join("llama-b11524/llama-server");
        assert_ne!(
            fs::metadata(&executable).unwrap().permissions().mode() & 0o111,
            0
        );
        assert_eq!(find_runtime_executable(&destination), Some(executable));
    }

    #[cfg(unix)]
    fn runtime_tar(path: &Path, files: &[(&str, &[u8], u32)], links: &[(&str, &str)]) {
        let archive = File::create(path).unwrap();
        let encoder = GzEncoder::new(archive, Compression::default());
        let mut builder = Builder::new(encoder);
        // Links first, as in the pinned archives, so extraction must defer them.
        for (link, target) in links {
            let mut header = Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_path(link).unwrap();
            header.set_link_name(target).unwrap();
            header.set_size(0);
            header.set_mode(0o777);
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        }
        for (name, bytes, mode) in files {
            let mut header = Header::new_gnu();
            header.set_path(name).unwrap();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder.append(&header, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[cfg(unix)]
    fn runtime_store(root: &Path) -> ModelStore {
        ModelStore::with_manifest(
            root,
            ModelManifest {
                schema_version: 1,
                models: Vec::new(),
                runtimes: vec![RuntimeDescriptor {
                    id: "test-runtime".into(),
                    name: "Test runtime".into(),
                    version: "test".into(),
                    platform: "test".into(),
                    files: Vec::new(),
                }],
            },
        )
        .unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn runtime_archives_with_versioned_library_links_install_and_verify() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.tar.gz");
        runtime_tar(
            &archive,
            &[
                ("llama-b1/llama-server", b"#!/bin/sh\n", 0o755),
                ("llama-b1/libllama.0.6.0.dylib", b"library", 0o644),
            ],
            &[
                ("llama-b1/libllama.dylib", "libllama.0.dylib"),
                ("llama-b1/libllama.0.dylib", "libllama.0.6.0.dylib"),
            ],
        );
        let store = runtime_store(temp.path());
        let root = temp.path().join("runtime/llama.cpp/test-runtime");
        fs::create_dir_all(&root).unwrap();
        extract_runtime_archive(&archive, &root).unwrap();
        assert_eq!(
            fs::read_link(root.join("llama-b1/libllama.0.dylib")).unwrap(),
            PathBuf::from("libllama.0.6.0.dylib")
        );
        assert_eq!(
            fs::read(root.join("llama-b1/libllama.dylib")).unwrap(),
            b"library"
        );

        // No install record yet: a half-extracted tree is not installed.
        assert!(!store.runtime_status("test-runtime").unwrap().installed);
        write_install_record("test-runtime", &root, vec!["archive-sha".into()]).unwrap();
        let status = store.runtime_status("test-runtime").unwrap();
        assert!(status.installed);
        let executable = store.verified_runtime_executable("test-runtime").unwrap();
        assert_eq!(executable, root.join("llama-b1/llama-server"));

        // A changed executable is refused before launch.
        fs::write(&executable, b"#!/bin/sh\necho changed\n").unwrap();
        assert!(store.verified_runtime_executable("test-runtime").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_archive_links_outside_their_directory_are_refused() {
        for target in ["../escape", "/etc/passwd", "sub/file", ".."] {
            let temp = tempfile::tempdir().unwrap();
            let archive = temp.path().join("runtime.tar.gz");
            runtime_tar(
                &archive,
                &[("llama-b1/llama-server", b"#!/bin/sh\n", 0o755)],
                &[("llama-b1/libllama.0.dylib", target)],
            );
            let destination = temp.path().join("runtime");
            fs::create_dir_all(&destination).unwrap();
            assert!(
                extract_runtime_archive(&archive, &destination).is_err(),
                "link target {target:?} was accepted"
            );
            assert!(fs::symlink_metadata(destination.join("llama-b1/libllama.0.dylib")).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn runtime_archive_links_to_missing_files_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("runtime.tar.gz");
        runtime_tar(
            &archive,
            &[("llama-b1/llama-server", b"#!/bin/sh\n", 0o755)],
            &[("llama-b1/libllama.0.dylib", "libllama.0.6.0.dylib")],
        );
        let destination = temp.path().join("runtime");
        fs::create_dir_all(&destination).unwrap();
        assert!(extract_runtime_archive(&archive, &destination).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn runtime_status_requires_an_executable_server() {
        let temp = tempfile::tempdir().unwrap();
        let store = ModelStore::with_manifest(
            temp.path(),
            ModelManifest {
                schema_version: 1,
                models: Vec::new(),
                runtimes: vec![RuntimeDescriptor {
                    id: "test-runtime".into(),
                    name: "Test runtime".into(),
                    version: "test".into(),
                    platform: "test".into(),
                    files: Vec::new(),
                }],
            },
        )
        .unwrap();
        let executable = temp
            .path()
            .join("runtime/llama.cpp/test-runtime/llama-server");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"not executable").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o644)).unwrap();

        let status = store.runtime_status("test-runtime").unwrap();
        assert!(!status.installed);
        assert!(status.executable_path.is_none());
    }

    #[test]
    fn embedded_manifest_is_pinned_and_valid() {
        let temp = tempfile::tempdir().unwrap();
        let store = ModelStore::new(temp.path()).unwrap();
        assert_eq!(store.manifest().schema_version, 1);
        assert!(store.model("multilingual-e5-small-int8").is_ok());
        assert!(store.runtime("llama-b11524-ubuntu-x64").is_ok());
    }
}
