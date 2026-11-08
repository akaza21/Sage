//! User-defined ACP agents. Sage holds four registrations, one per live
//! agent; each owns a static profile whose ids are internal. The agent's
//! own `custom-acp-<n>` id, which is never reused, is the harness and
//! binding id sessions see. What the user configured (executable,
//! arguments, passed-through variable names) is admitted as a launch
//! extension of the profile. The executable is launched directly, never
//! through a shell, and its digest is re-proved before every launch.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    domain::HarnessUnavailableReason,
    settings_store::{CustomAcpAgent, MAX_CUSTOM_ACP_AGENTS, custom_acp_number},
};

use super::{
    AdmittedStandardAcpBinding, StandardAcpArtifact, StandardAcpArtifactProvenance,
    StandardAcpBindingSource, StandardAcpCandidate,
    admission::{StandardAcpLaunchExtension, admit_extended},
    custom_executable::check_custom_executable,
    profile::{
        StandardAcpClientCapabilities, StandardAcpLaunch, StandardAcpLaunchPolicy,
        StandardAcpProfile, StandardAcpVersionShape,
    },
};

/// The access policy every user-defined agent shares.
pub(crate) const CUSTOM_ACP_POLICY_ID: &str = "custom-acp";
const NOTICE_CHARS: usize = 480;

const fn custom_profile(id: &'static str) -> StandardAcpProfile {
    StandardAcpProfile {
        harness_id: id,
        binding_id: id,
        launch: StandardAcpLaunch::DirectExecutable,
        fixed_args: &[],
        version_probe_args: &[],
        version_shape: StandardAcpVersionShape::Unprobed,
        // What Sage implements: approved file writes; reads are refused.
        client_capabilities: StandardAcpClientCapabilities {
            read_text_file: false,
            write_text_file: true,
        },
        package_manifest: None,
        other_tool_label: "Agent tool",
        command_input_field: None,
        task_tool_lifecycle: false,
        vendor_updates: &[],
        passthrough_env: &[],
        remediation: "Complete the agent's sign-in outside Sage, then try again.",
        agent_label: "The agent",
        session_mode: None,
        launch_policy: StandardAcpLaunchPolicy::Inherit,
        elicitation: true,
        authenticate_on_auth_required: true,
        close_characterised: false,
    }
}

pub(crate) static CUSTOM_PROFILES: [StandardAcpProfile; MAX_CUSTOM_ACP_AGENTS] = [
    custom_profile("custom-acp-slot-1"),
    custom_profile("custom-acp-slot-2"),
    custom_profile("custom-acp-slot-3"),
    custom_profile("custom-acp-slot-4"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomAcpUnavailable {
    Pending,
    NotConfigured,
    ExecutableInvalid,
}

#[derive(Debug)]
enum State {
    Pending,
    Empty,
    Configured {
        agent: CustomAcpAgent,
        binding: Result<Arc<AdmittedStandardAcpBinding>, CustomAcpUnavailable>,
    },
}

/// One live agent's admission. Interior state changes when the user saves
/// or removes an agent; the registry record stays fixed and answers to
/// whichever agent id the registration holds.
#[derive(Debug)]
pub(crate) struct CustomAcpRegistration {
    profile: &'static StandardAcpProfile,
    state: Mutex<State>,
    sign_in: Mutex<Option<String>>,
}

impl CustomAcpRegistration {
    pub(crate) fn startup(slot: usize) -> Option<Self> {
        Some(Self {
            profile: CUSTOM_PROFILES.get(slot.checked_sub(1)?)?,
            state: Mutex::new(State::Pending),
            sign_in: Mutex::new(None),
        })
    }

    pub(crate) fn profile(&self) -> &'static StandardAcpProfile {
        self.profile
    }

    /// Admits `agent` (or clears the registration), proving the executable
    /// is a launchable regular file and recording its digest. The same
    /// agent applied again keeps an admission it holds, and its sign-in
    /// notice; one that was refused is admitted afresh.
    pub(crate) fn apply(&self, agent: Option<&CustomAcpAgent>) {
        let mut state = self.lock();
        if let (
            State::Configured {
                agent: held,
                binding: Ok(_),
            },
            Some(agent),
        ) = (&*state, agent)
            && held == agent
        {
            return;
        }
        *state = match agent {
            Some(agent) if custom_acp_number(&agent.id).is_some() => State::Configured {
                binding: admit_agent(self.profile, agent),
                agent: agent.clone(),
            },
            _ => State::Empty,
        };
        drop(state);
        self.clear_sign_in();
    }

    /// The id of the agent this registration holds.
    pub(crate) fn agent_id(&self) -> Option<String> {
        match &*self.lock() {
            State::Configured { agent, .. } => Some(agent.id.clone()),
            State::Pending | State::Empty => None,
        }
    }

    pub(crate) fn serves(&self, id: &str) -> bool {
        matches!(&*self.lock(), State::Configured { agent, .. } if agent.id == id)
    }

    pub(crate) fn is_vacant(&self) -> bool {
        !matches!(&*self.lock(), State::Configured { .. })
    }

    fn clear_sign_in(&self) {
        *self.sign_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub(crate) fn display_name(&self) -> Option<String> {
        match &*self.lock() {
            State::Configured { agent, .. } => Some(agent.name.clone()),
            State::Pending | State::Empty => None,
        }
    }

    pub(crate) fn admitted(&self) -> Option<Arc<AdmittedStandardAcpBinding>> {
        match &*self.lock() {
            State::Configured {
                binding: Ok(binding),
                ..
            } => Some(Arc::clone(binding)),
            _ => None,
        }
    }

    pub(crate) fn unavailable(&self) -> Option<CustomAcpUnavailable> {
        match &*self.lock() {
            State::Pending => Some(CustomAcpUnavailable::Pending),
            State::Empty => Some(CustomAcpUnavailable::NotConfigured),
            State::Configured { binding, .. } => binding.as_ref().err().copied(),
        }
    }

    /// The admitted binding, re-proved now. The user chose a path, not a
    /// build: an executable rebuilt in place is admitted again by digest.
    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedStandardAcpBinding>, CustomAcpUnavailable> {
        let mut state = self.lock();
        let State::Configured { agent, binding } = &mut *state else {
            return Err(self.unavailable_of(&state));
        };
        if let Ok(current) = binding
            && current.revalidate().is_ok()
        {
            return Ok(Arc::clone(current));
        }
        *binding = admit_agent(self.profile, agent);
        binding.clone()
    }

    pub(crate) fn sign_in_notice(&self) -> Option<String> {
        self.sign_in
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn unavailable_of(&self, state: &State) -> CustomAcpUnavailable {
        match state {
            State::Pending => CustomAcpUnavailable::Pending,
            _ => CustomAcpUnavailable::NotConfigured,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| {
            let mut state = poisoned.into_inner();
            *state = State::Empty;
            state
        })
    }
}

impl StandardAcpBindingSource for CustomAcpRegistration {
    fn refresh_binding(&self) -> Option<Arc<AdmittedStandardAcpBinding>> {
        self.refresh().ok()
    }

    fn record_sign_in(&self, notice: &str) {
        let notice: String = notice.chars().take(NOTICE_CHARS).collect();
        *self.sign_in.lock().unwrap_or_else(|e| e.into_inner()) = Some(notice);
    }

    fn session_ready(&self) {
        self.clear_sign_in();
    }
}

impl CustomAcpUnavailable {
    pub(crate) fn reason(self) -> HarnessUnavailableReason {
        match self {
            Self::Pending => HarnessUnavailableReason::AdmissionIncomplete,
            Self::NotConfigured | Self::ExecutableInvalid => {
                HarnessUnavailableReason::CandidateInvalid
            }
        }
    }
}

fn admit_agent(
    profile: &'static StandardAcpProfile,
    agent: &CustomAcpAgent,
) -> Result<Arc<AdmittedStandardAcpBinding>, CustomAcpUnavailable> {
    let path = PathBuf::from(&agent.executable);
    let metadata =
        check_custom_executable(&path).map_err(|_| CustomAcpUnavailable::ExecutableInvalid)?;
    let candidate = StandardAcpCandidate {
        runtime: StandardAcpArtifact {
            sha256: crate::harness_discovery::sha256_file(&path)
                .ok_or(CustomAcpUnavailable::ExecutableInvalid)?,
            byte_len: metadata.len(),
            path,
            provenance: StandardAcpArtifactProvenance::Direct,
        },
        entrypoint: None,
        package_manifest: None,
        reported_version: String::new(),
    };
    let extension = StandardAcpLaunchExtension {
        args: agent.args.clone(),
        passthrough_env: agent.env.clone(),
    };
    admit_extended(profile, &agent.id, candidate, extension)
        .map_err(|_| CustomAcpUnavailable::ExecutableInvalid)
}

#[cfg(test)]
#[path = "custom_tests.rs"]
mod tests;
