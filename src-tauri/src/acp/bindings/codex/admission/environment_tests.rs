use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::{AdmittedCodexBinding, CodexAdmissionUnavailable, ObservedLaunchInputs};

#[derive(Debug, Clone)]
struct LaunchState {
    variables: BTreeMap<String, String>,
    config: Result<Option<Vec<u8>>, CodexAdmissionUnavailable>,
    node: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct TestLaunchInputs(Mutex<LaunchState>);

impl Default for TestLaunchInputs {
    fn default() -> Self {
        let home = std::env::temp_dir();
        #[cfg(windows)]
        let windows = super::super::authority::system_windows_directory_for_test().unwrap();
        #[cfg(not(windows))]
        let windows = PathBuf::from("/test/windows");
        let variables = [
            ("APPDATA", home.clone()),
            ("ComSpec", windows.join("System32").join("cmd.exe")),
            ("SystemRoot", windows.clone()),
            ("TEMP", home.clone()),
            ("TMP", home.clone()),
            ("USERPROFILE", home.clone()),
            ("WINDIR", windows),
            ("HOME", home),
        ]
        .into_iter()
        .map(|(key, path)| (key.to_owned(), path.to_string_lossy().into_owned()))
        .chain([("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into())])
        .collect();
        Self(Mutex::new(LaunchState {
            variables,
            config: Ok(Some(b"model_provider = \"openai\"".to_vec())),
            node: Some(PathBuf::from("C:/fixture-node")),
        }))
    }
}

impl ObservedLaunchInputs for TestLaunchInputs {
    fn variable(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().variables.get(key).cloned()
    }

    fn config(&self) -> Result<Option<Vec<u8>>, CodexAdmissionUnavailable> {
        self.0.lock().unwrap().config.clone()
    }

    fn node_directory(&self, _: &Path) -> Option<PathBuf> {
        self.0.lock().unwrap().node.clone()
    }
}

impl AdmittedCodexBinding {
    pub(crate) fn admit_observed_for_test(
        executable: PathBuf,
        version: String,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        Self::admit_observed_with_inputs(executable, version, Arc::new(TestLaunchInputs::default()))
    }
}

impl super::CodexBindingRegistration {
    pub(crate) fn authenticate_observed_for_test(
        &self,
        executable: PathBuf,
        version: String,
    ) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        self.authenticate_observed_with_inputs(
            executable,
            version,
            Arc::new(TestLaunchInputs::default()),
        )
    }
}

#[cfg(windows)]
struct Executable(PathBuf);

#[cfg(windows)]
impl Executable {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-admission-{}.cmd", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"@echo off\r\n").unwrap();
        Self(path)
    }

    fn admit(
        &self,
        inputs: Arc<TestLaunchInputs>,
    ) -> Result<AdmittedCodexBinding, CodexAdmissionUnavailable> {
        AdmittedCodexBinding::admit_observed_with_inputs(
            self.0.clone(),
            crate::acp::bindings::codex::test_identity::version_line("codex"),
            inputs,
        )
    }
}

#[cfg(windows)]
impl Drop for Executable {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn observed_admission_and_revalidation_accept_builtin_openai_without_a_table() {
    let executable = Executable::new();
    let binding = executable
        .admit(Arc::new(TestLaunchInputs::default()))
        .unwrap();
    assert_eq!(
        binding.launch_environment()[0],
        ("PATH".to_owned(), "C:/fixture-node".to_owned())
    );
    binding.revalidate().unwrap();
}

#[cfg(windows)]
#[test]
fn observed_custom_provider_forwards_declared_keys_and_revalidates_their_values() {
    let executable = Executable::new();
    let inputs = Arc::new(TestLaunchInputs::default());
    {
        let mut state = inputs.0.lock().unwrap();
        state.config = Ok(Some(
            br#"
model_provider = "custom"
[model_providers.custom]
env_key = "CUSTOM_API_KEY"
env_http_headers = { "X-Secondary" = "SECONDARY_KEY" }
[model_providers.other]
env_key = "OTHER_API_KEY"
"#
            .to_vec(),
        ));
        for (key, value) in [
            ("CUSTOM_API_KEY", "custom-secret"),
            ("SECONDARY_KEY", "secondary-secret"),
            ("OTHER_API_KEY", "unselected-secret"),
        ] {
            state.variables.insert(key.into(), value.into());
        }
    }
    let binding = executable.admit(Arc::clone(&inputs)).unwrap();
    assert_eq!(
        &binding.launch_environment()[9..],
        &[
            ("CUSTOM_API_KEY".to_owned(), "custom-secret".to_owned()),
            ("SECONDARY_KEY".to_owned(), "secondary-secret".to_owned()),
        ]
    );
    binding.revalidate().unwrap();
    inputs
        .0
        .lock()
        .unwrap()
        .variables
        .insert("CUSTOM_API_KEY".to_owned(), "changed-secret".into());
    assert_eq!(binding.revalidate(), Err(CodexAdmissionUnavailable::Drift));
}

#[cfg(windows)]
#[test]
fn observed_admission_and_revalidation_reject_an_undefined_provider() {
    let executable = Executable::new();
    let inputs = Arc::new(TestLaunchInputs::default());
    let binding = executable.admit(Arc::clone(&inputs)).unwrap();
    inputs.0.lock().unwrap().config = Ok(Some(b"model_provider = \"unknown\"".to_vec()));
    assert_eq!(
        executable.admit(Arc::clone(&inputs)).unwrap_err(),
        CodexAdmissionUnavailable::IndependentAdmission
    );
    assert_eq!(
        binding.revalidate(),
        Err(CodexAdmissionUnavailable::IndependentAdmission)
    );
}

#[cfg(windows)]
#[test]
fn revalidation_rereads_config_environment_and_node_resolution_from_injected_inputs() {
    let executable = Executable::new();
    let inputs = Arc::new(TestLaunchInputs::default());
    let original = inputs.0.lock().unwrap().clone();
    let binding = executable.admit(Arc::clone(&inputs)).unwrap();
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.node = Some(PathBuf::from("C:/different-node"));
    changes.push(changed);
    let mut changed = original.clone();
    changed
        .variables
        .insert("APPDATA".into(), "C:/different-appdata".into());
    changes.push(changed);
    let mut changed = original.clone();
    changed.config = Ok(Some(
        br#"model_provider = "openai"
[model_providers.openai]
env_key = "ADDED_API_KEY"
"#
        .to_vec(),
    ));
    changed
        .variables
        .insert("ADDED_API_KEY".into(), "added-secret".into());
    changes.push(changed);
    for changed in changes {
        *inputs.0.lock().unwrap() = changed;
        assert_eq!(binding.revalidate(), Err(CodexAdmissionUnavailable::Drift));
        *inputs.0.lock().unwrap() = original.clone();
        binding.revalidate().unwrap();
    }
}

#[cfg(windows)]
#[test]
fn observed_admission_fails_closed_on_missing_launch_inputs_and_bad_config() {
    let executable = Executable::new();
    let inputs = Arc::new(TestLaunchInputs::default());
    let original = inputs.0.lock().unwrap().clone();
    let mut changes = Vec::new();
    let mut changed = original.clone();
    changed.node = None;
    changes.push(changed);
    let mut changed = original.clone();
    changed.variables.remove("USERPROFILE");
    changes.push(changed);
    for config in [
        Err(CodexAdmissionUnavailable::IndependentAdmission),
        Ok(Some(vec![b'x'; 64 * 1024 + 1])),
        Ok(Some(vec![0xff])),
        Ok(Some(b"model_provider = [".to_vec())),
    ] {
        let mut changed = original.clone();
        changed.config = config;
        changes.push(changed);
    }
    for changed in changes {
        *inputs.0.lock().unwrap() = changed;
        assert_eq!(
            executable.admit(Arc::clone(&inputs)).unwrap_err(),
            CodexAdmissionUnavailable::IndependentAdmission
        );
    }
    inputs.0.lock().unwrap().clone_from(&original);
    inputs.0.lock().unwrap().config = Ok(None);
    executable.admit(inputs).unwrap().revalidate().unwrap();
}
