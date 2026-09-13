//! SUPERVISOR-DESCENDANT-SETTLEMENT: accounting and object signaling are distinct.
use std::{
    fs,
    io::Read,
    os::windows::io::AsRawHandle,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, WaitForSingleObject,
    },
};

use super::native::NativeHandle;
use crate::{
    acp::{
        contract::{ProjectGeneration, SageProjectId, SageRootId},
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchSpec, PlatformSupervisor,
            ProcessDeadlines, RequiredContainment, TreeCapacityPool, TreeExit, TreeLimits,
        },
    },
    project_registry::ProjectRootAuthority,
};

const HELPER: &str = "acp::process::windows::descendant_settlement_tests::non_app_server_child";

pub(super) fn membership_snapshot(job: HANDLE) -> (u32, u32) {
    use windows_sys::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, QueryInformationJobObject,
    };
    #[repr(C)]
    struct Snapshot {
        assigned: u32,
        returned: u32,
        ids: [usize; 33],
    }
    let mut snapshot = Snapshot {
        assigned: 0,
        returned: 0,
        ids: [0; 33],
    };
    // SAFETY: repr(C) header and bounded 33-entry storage match the variable-length API structure.
    assert_ne!(
        unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicProcessIdList,
                (&raw mut snapshot).cast(),
                std::mem::size_of_val(&snapshot) as u32,
                std::ptr::null_mut(),
            )
        },
        0
    );
    (snapshot.assigned, snapshot.returned)
}

fn creation_identity(handle: HANDLE) -> u64 {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: callers retain query-capable handles; all outputs are writable.
    assert_ne!(
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) },
        0
    );
    (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)
}

#[test]
#[ignore = "subprocess helper invoked only by the retained-handle diagnostic"]
fn non_app_server_child() {
    match std::env::var("SAGE_SETTLEMENT_MODE")
        .unwrap_or_default()
        .as_str()
    {
        "sleep" => {
            thread::sleep(Duration::from_secs(30));
            return;
        }
        "success" => {}
        _ => return, // A direct `cargo test -- --ignored` invocation is a no-op.
    }
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", HELPER, "--ignored", "--nocapture"])
        .env("SAGE_SETTLEMENT_MODE", "sleep")
        .spawn()
        .unwrap();
    // The creator's retained handle supplies the identity, not a later PID lookup.
    fs::write(
        "descendant.receipt.tmp",
        format!(
            "{} {}",
            child.id(),
            creation_identity(child.as_raw_handle())
        ),
    )
    .unwrap();
    fs::rename("descendant.receipt.tmp", "descendant.receipt").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::path::Path::new("release-root").exists() {
        assert!(
            Instant::now() < deadline,
            "parent must retain and verify the child before root exit"
        );
        thread::sleep(Duration::from_millis(1));
    }
    println!("1.2.3");
    std::process::exit(0);
}

#[test]
fn non_app_server_token_proves_root_exit_and_job_zero_not_descendant_settlement() {
    let directory = std::env::temp_dir().join(format!("sage-settlement-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let path = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut file = fs::File::open(&path).unwrap();
    let bytes = file.metadata().unwrap().len();
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let spec = LaunchSpec {
        binding_id: Uuid::nil(),
        runtime_epoch: 901,
        artifact: ArtifactExpectation::new(Uuid::nil(), path, hasher.finalize().into(), bytes)
            .unwrap(),
        fixed_args: BoundedOsArgs::new(
            ["--exact", HELPER, "--ignored", "--nocapture"]
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .unwrap(),
        environment: BoundedEnvironment::new(vec![(
            "SAGE_SETTLEMENT_MODE".into(),
            "success".into(),
        )])
        .unwrap(),
        project_root: ProjectRootAuthority::for_test(
            SageProjectId::parse("project-settlement").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-settlement").unwrap(),
            &directory,
        )
        .unwrap(),
        required_guarantee: RequiredContainment::EnforcedTree,
        limits: TreeLimits::ade_0006a(),
        deadlines: ProcessDeadlines::ade_0006a(),
        absolute_cleanup_deadline: None,
        test_after_artifact_verified: None,
    };
    let pool = TreeCapacityPool::isolated_ade_0006a();
    let tree =
        PlatformSupervisor::launch(&spec, pool.try_acquire(spec.limits.descendants).unwrap())
            .unwrap();
    let (mut stdin, mut stdout, control) = tree.into_parts();
    stdin.close();
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0_u8; 256];
        loop {
            let read = stdout.read(&mut chunk).unwrap();
            if read == 0 {
                return output;
            }
            output.extend_from_slice(&chunk[..read]);
            assert!(output.len() <= 4096);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let receipt = loop {
        if let Ok(receipt) = fs::read_to_string(directory.join("descendant.receipt")) {
            break receipt;
        }
        assert!(Instant::now() < deadline, "bounded descendant readiness");
        thread::sleep(Duration::from_millis(1));
    };
    let receipt: Vec<u64> = receipt
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    // Open BEFORE releasing the root, then retain this same handle through convergence.
    let descendant = NativeHandle::new(unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            receipt[0] as u32,
        )
    })
    .unwrap();
    let creation = creation_identity(descendant.raw());
    assert_eq!(
        creation, receipt[1],
        "must match the creator's exact child identity"
    );
    assert!(
        control.contains_exact_handle(descendant.raw()),
        "child must be in this exact Job"
    );
    assert_eq!(
        unsafe { WaitForSingleObject(descendant.raw(), 0) },
        WAIT_TIMEOUT
    );
    assert_eq!(control.active_processes().unwrap(), 2);
    assert_eq!(control.total_processes().unwrap(), 2);
    // The root is held by our handshake: this is ordering, not a timing threshold.
    assert_eq!(
        control.wait_zero_within(Duration::ZERO).err(),
        Some(crate::acp::process::LaunchError::Deadline)
    );
    assert_ne!(pool.usage(), (0, 0));
    fs::write(directory.join("release-root"), b"release").unwrap();
    assert_eq!(
        control.wait_root_exit().unwrap(),
        TreeExit::Exited { code: Some(0) }
    );
    let started = Instant::now();
    let (exit, token) = control.wait_zero().unwrap();
    let convergence = started.elapsed();
    assert_eq!(token.runtime_epoch(), spec.runtime_epoch);
    let wait_at_token = unsafe { WaitForSingleObject(descendant.raw(), 0) };
    let membership = control.settled_root_and_stderr_membership_snapshot();
    let wait_after_membership = unsafe { WaitForSingleObject(descendant.raw(), 0) };
    eprintln!(
        "SETTLEMENT membership: assigned={} returned={} descendant_wait_after_membership={wait_after_membership}",
        membership.0, membership.1
    );
    let active = control.active_processes().unwrap();
    let total = control.total_processes().unwrap();
    let usage = pool.usage();
    let same_identity = creation_identity(descendant.raw()) == creation;
    eprintln!(
        "SETTLEMENT receipt: exact_job_member=true creator_identity_matches=true retained_identity_unchanged={same_identity} root_exit={exit:?} epoch={} job_active={active} job_total={total} pool={usage:?} descendant_wait_at_token={wait_at_token} convergence_us={}",
        token.runtime_epoch(),
        convergence.as_micros()
    );
    // Test-owned handle, not production authority. Explicitly wait before deleting
    // the fixture directory; the token alone is not a filesystem cleanup barrier.
    // Do not assert a settlement-time bound. The test-owned handle remains live
    // through this wait even if kernel cleanup outlasts the fixture's own sleep.
    let cleanup_wait = unsafe { WaitForSingleObject(descendant.raw(), INFINITE) };
    assert_eq!(cleanup_wait, WAIT_OBJECT_0);
    eprintln!(
        "SETTLEMENT cleanup: wait={cleanup_wait} elapsed_us={}",
        started.elapsed().as_micros()
    );
    let output = reader.join().unwrap();
    assert!(String::from_utf8_lossy(&output).contains("1.2.3"));
    drop(control);
    drop(stdin);
    drop(descendant);
    fs::remove_dir_all(directory).unwrap();
    assert_eq!(exit, TreeExit::Exited { code: Some(0) });
    assert_eq!(usage, (0, 0));
    assert_eq!(active, 0);
    assert_eq!(total, 2);
    assert!(
        same_identity,
        "retained handle must exclude PID reuse throughout convergence"
    );
    assert_eq!(membership, (0, 0));
    // Either observation is valid. Requiring WAIT_TIMEOUT would make this a
    // scheduler-dependent race; requiring WAIT_OBJECT_0 would overstate the token.
    assert!(matches!(wait_at_token, WAIT_OBJECT_0 | WAIT_TIMEOUT));
    assert!(matches!(
        wait_after_membership,
        WAIT_OBJECT_0 | WAIT_TIMEOUT
    ));
}
