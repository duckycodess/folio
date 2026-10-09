use crate::contracts::{
    ModelDescriptor, ModelFile, ModelInstallState, ModelInstallStatus, ModelRole,
    NativeProviderError, ProviderErrorCode,
};
use crate::error::{CoreError, CoreResult, NativeProviderErrorError};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tar::Archive;
use zip::ZipArchive;

const MANIFEST_JSON: &str = include_str!("../../../resources/model-manifest.json");
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

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
            match verify_file(&path, file) {
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
        self.model_state(id)
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
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(DOWNLOAD_TIMEOUT)
            .build()?;
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
            let mut response = client.get(url).send()?.error_for_status()?;
            let mut output = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&partial)?;
            let mut hasher = Sha256::new();
            let mut received = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    let _ = fs::remove_file(&partial);
                    return Err(provider(
                        ProviderErrorCode::Cancelled,
                        "Model installation cancelled.",
                    ));
                }
                let count = response.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
                hasher.update(&buffer[..count]);
                received += count as u64;
                on_progress(DownloadProgress {
                    item_id: id.into(),
                    file: file.path.clone(),
                    received_bytes: received,
                    total_bytes: file.bytes,
                });
            }
            output.flush()?;
            output.sync_all()?;
            let digest = hex::encode(hasher.finalize());
            if received != file.bytes || digest != file.sha256 {
                let _ = fs::remove_file(&partial);
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    format!(
                        "Downloaded {} failed size or SHA-256 verification.",
                        file.path
                    ),
                ));
            }
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
        Ok(())
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
        let settings_path = self.data_dir.join("settings.json");
        fs::create_dir_all(&self.data_dir)?;
        let mut settings = if settings_path.exists() {
            serde_json::from_str::<ModelSettings>(&fs::read_to_string(&settings_path)?)?
        } else {
            ModelSettings {
                embedding_model_id: None,
                generation_model_id: None,
            }
        };
        match role {
            ModelRole::Embedding => settings.embedding_model_id = Some(id.into()),
            ModelRole::Generation => settings.generation_model_id = Some(id.into()),
        }
        let temporary = settings_path.with_extension("json.partial");
        let mut file = File::create(&temporary)?;
        serde_json::to_writer_pretty(&mut file, &settings)?;
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

    pub fn runtime_status(&self, id: &str) -> CoreResult<RuntimeStatus> {
        let runtime = self.runtime(id)?;
        let root = self.runtime_root(id)?;
        let executable_path = find_runtime_executable(&root);
        Ok(RuntimeStatus {
            id: id.into(),
            version: runtime.version.clone(),
            installed: executable_path.is_some(),
            executable_path,
        })
    }

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
        fs::create_dir_all(&root)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(DOWNLOAD_TIMEOUT)
            .build()?;
        for file in descriptor.files {
            let url = file.download_url.clone().ok_or_else(|| {
                CoreError::Message(format!("Manifest has no download URL for {}.", file.path))
            })?;
            let archive = safe_join(&root, &file.path)?;
            let partial = partial_path(&archive);
            if is_symlink_or_inside_symlink(&root, &archive)?
                || is_symlink_or_inside_symlink(&root, &partial)?
            {
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    "Refusing to write through a runtime archive symlink.",
                ));
            }
            let mut response = client.get(url).send()?.error_for_status()?;
            let mut output = File::create(&partial)?;
            let mut hasher = Sha256::new();
            let mut received = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    let _ = fs::remove_file(&partial);
                    return Err(provider(
                        ProviderErrorCode::Cancelled,
                        "Runtime installation cancelled.",
                    ));
                }
                let count = response.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                output.write_all(&buffer[..count])?;
                hasher.update(&buffer[..count]);
                received += count as u64;
                on_progress(DownloadProgress {
                    item_id: id.into(),
                    file: file.path.clone(),
                    received_bytes: received,
                    total_bytes: file.bytes,
                });
            }
            output.flush()?;
            output.sync_all()?;
            if received != file.bytes || hex::encode(hasher.finalize()) != file.sha256 {
                let _ = fs::remove_file(&partial);
                return Err(provider(
                    ProviderErrorCode::ModelCorrupt,
                    format!("Downloaded runtime {} failed verification.", file.path),
                ));
            }
            fs::rename(partial, &archive)?;
            extract_runtime_archive(&archive, &root)?;
            fs::remove_file(archive)?;
        }
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

fn provider(code: ProviderErrorCode, message: impl Into<String>) -> CoreError {
    CoreError::Provider(NativeProviderErrorError::new(code, message))
}

fn validate_id(id: &str) -> CoreResult<()> {
    if id.is_empty()
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

fn extract_runtime_archive(archive: &Path, destination: &Path) -> CoreResult<()> {
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
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
                return Err(CoreError::Archive(
                    "Runtime archive contains a symlink.".into(),
                ));
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
        return Ok(());
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
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(CoreError::Archive(
                "Runtime archive contains a link.".into(),
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
    }
    Ok(())
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
