use std::{
    ffi::c_void,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
};

use super::{InstalledAuthenticodePolicy, VerifyError};

const WTD_UI_NONE: u32 = 2;
const WTD_REVOKE_NONE: u32 = 0;
const WTD_CHOICE_FILE: u32 = 1;
const WTD_STATEACTION_VERIFY: u32 = 1;
const WTD_STATEACTION_CLOSE: u32 = 2;
const CERT_SHA1_HASH_PROP_ID: u32 = 3;
const CERT_NAME_ATTR_TYPE: u32 = 3;
const CERTIFICATE_SHA1_BYTES: usize = 20;
const SUBJECT_NAME_MAX: usize = 512;
const COMMON_NAME_OID: &[u8] = b"2.5.4.3\0";

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const WINTRUST_ACTION_GENERIC_VERIFY_V2: Guid = Guid {
    data1: 0x00aac56b,
    data2: 0xcd44,
    data3: 0x11d0,
    data4: [0x8c, 0xc2, 0x00, 0xc0, 0x4f, 0xc2, 0x95, 0xee],
};

#[repr(C)]
struct WintrustFileInfo {
    cb_struct: u32,
    file_path: *const u16,
    file_handle: *mut c_void,
    known_subject: *const Guid,
}

#[repr(C)]
struct WintrustData {
    cb_struct: u32,
    policy_callback_data: *mut c_void,
    sip_client_data: *mut c_void,
    ui_choice: u32,
    revocation_checks: u32,
    union_choice: u32,
    file_info: *mut WintrustFileInfo,
    state_action: u32,
    state_data: *mut c_void,
    url_reference: *const u16,
    provider_flags: u32,
    ui_context: u32,
    signature_settings: *mut c_void,
}

#[repr(C)]
struct CryptProviderData {
    _private: [u8; 0],
}

#[repr(C)]
struct CryptProviderCert {
    cb_struct: u32,
    certificate: *const CertContext,
}

#[repr(C)]
struct CryptProviderSigner {
    cb_struct: u32,
    verify_as_of_low: u32,
    verify_as_of_high: u32,
    certificate_chain_count: u32,
    certificate_chain: *mut CryptProviderCert,
    signer_type: u32,
    signer_info: *mut c_void,
    error: u32,
    countersigner_count: u32,
    countersigners: *mut CryptProviderSigner,
    chain_context: *mut c_void,
}

#[repr(C)]
struct CertContext {
    _private: [u8; 0],
}

#[link(name = "wintrust")]
unsafe extern "system" {
    fn WinVerifyTrust(
        window: *mut c_void,
        action_id: *const Guid,
        trust_data: *mut WintrustData,
    ) -> i32;
    fn WTHelperProvDataFromStateData(state_data: *mut c_void) -> *mut CryptProviderData;
    fn WTHelperGetProvSignerFromChain(
        provider_data: *mut CryptProviderData,
        signer_index: u32,
        countersigner: i32,
        countersigner_index: u32,
    ) -> *mut CryptProviderSigner;
    fn WTHelperGetProvCertFromChain(
        signer: *mut CryptProviderSigner,
        certificate_index: u32,
    ) -> *mut CryptProviderCert;
}

#[link(name = "crypt32")]
unsafe extern "system" {
    fn CertGetCertificateContextProperty(
        certificate: *const CertContext,
        property_id: u32,
        data: *mut c_void,
        data_len: *mut u32,
    ) -> i32;
    fn CertGetNameStringW(
        certificate: *const CertContext,
        name_type: u32,
        flags: u32,
        type_parameter: *const c_void,
        name: *mut u16,
        name_len: u32,
    ) -> u32;
}

pub(super) fn verify_file(
    path: &Path,
    policy: InstalledAuthenticodePolicy,
) -> Result<(), VerifyError> {
    let file = super::open_artifact_for_verification(path)?;
    let mut path_wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if path_wide.contains(&0) {
        return Err(VerifyError::Artifact);
    }
    path_wide.push(0);

    let mut file_info = WintrustFileInfo {
        cb_struct: size_of::<WintrustFileInfo>() as u32,
        file_path: path_wide.as_ptr(),
        file_handle: file.as_raw_handle().cast(),
        known_subject: std::ptr::null(),
    };
    let mut trust_data = WintrustData {
        cb_struct: size_of::<WintrustData>() as u32,
        policy_callback_data: std::ptr::null_mut(),
        sip_client_data: std::ptr::null_mut(),
        ui_choice: WTD_UI_NONE,
        revocation_checks: WTD_REVOKE_NONE,
        union_choice: WTD_CHOICE_FILE,
        file_info: &mut file_info,
        state_action: WTD_STATEACTION_VERIFY,
        state_data: std::ptr::null_mut(),
        url_reference: std::ptr::null(),
        provider_flags: 0,
        ui_context: 0,
        signature_settings: std::ptr::null_mut(),
    };

    let verified = unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &WINTRUST_ACTION_GENERIC_VERIFY_V2,
            &mut trust_data,
        ) == 0
    } && signer_matches_policy(trust_data.state_data, policy);

    let closed = close_verification_state(&mut trust_data);
    if verified && closed {
        Ok(())
    } else {
        Err(VerifyError::Authenticode)
    }
}

fn close_verification_state(trust_data: &mut WintrustData) -> bool {
    if trust_data.state_data.is_null() {
        return true;
    }
    trust_data.state_action = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            &WINTRUST_ACTION_GENERIC_VERIFY_V2,
            trust_data,
        ) == 0
    }
}

fn signer_matches_policy(state_data: *mut c_void, policy: InstalledAuthenticodePolicy) -> bool {
    if state_data.is_null() {
        return false;
    }
    let provider = unsafe { WTHelperProvDataFromStateData(state_data) };
    if provider.is_null() {
        return false;
    }
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return false;
    }
    let certificate = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if certificate.is_null() {
        return false;
    }
    let certificate = unsafe { (*certificate).certificate };
    if certificate.is_null() {
        return false;
    }
    certificate_thumbprint_matches(certificate, policy)
        && certificate_subject_matches(certificate, policy)
}

fn certificate_thumbprint_matches(
    certificate: *const CertContext,
    policy: InstalledAuthenticodePolicy,
) -> bool {
    let Some(expected) = decode_sha1(policy.certificate_thumbprint_sha1()) else {
        return false;
    };
    let mut observed = [0_u8; CERTIFICATE_SHA1_BYTES];
    let mut observed_len = observed.len() as u32;
    let has_thumbprint = unsafe {
        CertGetCertificateContextProperty(
            certificate,
            CERT_SHA1_HASH_PROP_ID,
            observed.as_mut_ptr().cast(),
            &mut observed_len,
        ) != 0
    };
    has_thumbprint
        && observed_len as usize == observed.len()
        && observed
            .iter()
            .zip(expected)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

fn certificate_subject_matches(
    certificate: *const CertContext,
    policy: InstalledAuthenticodePolicy,
) -> bool {
    let required = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_ATTR_TYPE,
            0,
            COMMON_NAME_OID.as_ptr().cast(),
            std::ptr::null_mut(),
            0,
        )
    } as usize;
    if required == 0 || required > SUBJECT_NAME_MAX {
        return false;
    }
    let mut subject = [0_u16; SUBJECT_NAME_MAX];
    let written = unsafe {
        CertGetNameStringW(
            certificate,
            CERT_NAME_ATTR_TYPE,
            0,
            COMMON_NAME_OID.as_ptr().cast(),
            subject.as_mut_ptr(),
            required as u32,
        )
    } as usize;
    if written != required || subject[required - 1] != 0 {
        return false;
    }
    std::char::decode_utf16(subject[..required - 1].iter().copied())
        .collect::<Result<String, _>>()
        .is_ok_and(|actual| actual.eq_ignore_ascii_case(policy.publisher_common_name()))
}

fn decode_sha1(value: &str) -> Option<[u8; CERTIFICATE_SHA1_BYTES]> {
    if value.len() != CERTIFICATE_SHA1_BYTES * 2 {
        return None;
    }
    let mut result = [0_u8; CERTIFICATE_SHA1_BYTES];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        result[index] = (hex(pair[0])? << 4) | hex(pair[1])?;
    }
    Some(result)
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
