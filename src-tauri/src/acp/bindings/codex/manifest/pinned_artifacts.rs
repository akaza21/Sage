//! The entrypoint comes first; the byte lengths sum to the package's declared
//! unpacked size.

#[derive(Clone, Copy)]
pub(super) struct ExpectedArtifact {
    pub(super) relative_path: &'static str,
    pub(super) sha256: &'static str,
    pub(super) byte_len: u64,
}

const fn artifact(
    relative_path: &'static str,
    sha256: &'static str,
    byte_len: u64,
) -> ExpectedArtifact {
    ExpectedArtifact {
        relative_path,
        sha256,
        byte_len,
    }
}

pub(in crate::acp::bindings::codex) const PINNED_ARTIFACT_COUNT: usize = PINNED_ARTIFACTS.len();

pub(super) const PINNED_ARTIFACTS: [ExpectedArtifact; 47] = [
    artifact(
        r"vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        "edfdd48f8b458af0906e507b4cb05217b326c3d6e515e9f024e05e3b8d650f70",
        369_062_704,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\bin\codex-code-mode-host.exe",
        "5acd42f62966dd4a498ebfe04c5cae4c4a644a342a65a21bb53f8f36235c3d35",
        80_328_496,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-command-runner.exe",
        "798919649d3069dd6c16292d73f3a5e5239a69843bf4b7ca6e77f412ce6e0d82",
        8_946_992,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-windows-sandbox-setup.exe",
        "6c55bfe87b35de7842c8025c6a9c041f682fcc5a80d26095579b50b4fa4ba57e",
        19_564_848,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-path\rg.exe",
        "14231169855ec5205cf5a1b6f1db358ff4aed4247c86b69ce8aae647c77f6680",
        4_218_880,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-package.json",
        "1052a791252f14520d3b218c0d0b7a2555c24b4f7b8d60c5b4694c9b4c6787f1",
        206,
    ),
    artifact(
        r"package.json",
        "3186d52ec2804e071cd1b6ec6a48330489fd98d3b31cc60d949963d367b118ec",
        511,
    ),
    artifact(
        r"README.md",
        "ba4e1f69ff48386e72a9c5e1edaf76aad64a475c2d51af79ccba6d1128261ba7",
        3_334,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\NOTICE.md",
        "473fcf8ccb68ce726111d016d133f0555adba730d057591512f3f71fb46aeb6e",
        2_439,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\codex-voice-host.exe",
        "2cbcc2976ed609a939bc6f84b0fc7be9bba43c8b728bb515dbb2283cb29b9613",
        9_028_400,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gio-2.0-0.dll",
        "d9af08fee6324e07dfbf6451dfe918401f3c151a8c2c79802d83ab90027db160",
        1_695_536,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\glib-2.0-0.dll",
        "9ed64991e24be3b3d748419317ad3dd78b52564d80f65d5231d013f6fede0893",
        1_390_896,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gmodule-2.0-0.dll",
        "fff1be2624c155a26ed6bff17760755cdc7c4e4094909e7b9f20ae7aa101dbd8",
        34_608,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gobject-2.0-0.dll",
        "6532e767f62a030284ea27e2c7140cdded46fe15dac8a54cd244bf007653cbcc",
        348_464,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstapp-1.0-0.dll",
        "a4cdfc311ac5acbb59e1f1e618b73962c1ac83ec60004eebdb6b7ca37c9d23d6",
        100_144,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstapp.dll",
        "1b9c6ab1d8697e3a6ef032f3d1100a984200da29bc7e7ad681657ecfa2273ba7",
        25_392,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudio-1.0-0.dll",
        "48a9d9fc8a791119ec53a0cc9e931754355f1b7010325410fb9c8ac76c88d3d7",
        512_304,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudioconvert.dll",
        "7729f0ada4951b5f136d81a9c6faaba7813b8d8c0bbf632e5820f03a0094e5dd",
        48_432,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudioresample.dll",
        "9d590b6bcab16c4d74597c3826549e1b9be042963f62822a70249ca4bce54161",
        45_360,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstbase-1.0-0.dll",
        "b438cfbcd5f4bc98e37481ffc2def2cf074942e90ccd211d6cf5c44621942339",
        484_144,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstcoreelements.dll",
        "595f233f7301ae31c21082c2f8e61b605efe8c551e4f10802cc5cbdd7980c1ee",
        409_904,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstnet-1.0-0.dll",
        "741926748c722edbe3b90d065cb7a69518956e4115019e0fa061d13ae0b8942f",
        116_528,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstopus.dll",
        "4850af98ea6c3de77832abdacff6ae88fa349af6bce9a2946eeeb5d6bd205c82",
        69_424,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstpbutils-1.0-0.dll",
        "c0c14eb754913727964d5d7b90e028b29ce7ac55742bb30ecd835a35b40f1851",
        262_448,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstreamer-1.0-0.dll",
        "783c9ddde5f33af15a6b7a26804e4dec2c3c79a3ff9033787bb364c1451c4761",
        1_257_264,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtp-1.0-0.dll",
        "9a7028d3fd38d3bee9f205fc43b22a47e193defc57cbf72cdc2c5b3cb4842392",
        167_216,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtp.dll",
        "f96f9d45aa08dd91018c2efc2efef38a7c742818005ad81fd95eb89caa5c7211",
        607_536,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtpmanager.dll",
        "cd98dd1bafa5f26a784635fdf707756393e4b129c0ecaa78d91ea4a1681ecf94",
        501_040,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gsttag-1.0-0.dll",
        "62d2e1d19382c6b92206afddc19c2141e25ec5655d139f712ed98ccd385cb1a9",
        254_256,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstvideo-1.0-0.dll",
        "198e2ba6353f41129d411b82c44255946bfd43cc52b030930e0a6c985d2015ec",
        769_840,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\intl-8.dll",
        "041fecb1945389b01d406f05bfcc807304e7a5989acc95212e959b14428f1c76",
        25_392,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\libffi-8.dll",
        "5d49364ef5b8724367016eceb186360e35074381e510a687f9369fd04e78dfc9",
        43_312,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\opus.dll",
        "02fe48770b20147667ccc5850797a8dd16a9f1e21800091f807780321faf3966",
        440_112,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\pcre2-8.dll",
        "5c2956bd3f0b0a74ffe55f4e4af08df51d3aaa1031d32b8e3519c7dbe9097025",
        398_128,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\vcruntime140.dll",
        "184146852727a9db4eea06178716bec3cdbb1015c911f6b0f915b184ad7775b2",
        123_472,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\z.dll",
        "f4956d397b9ac23262736781d603f0be5d650a5427aef2c50065de3cc94e39ae",
        105_264,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\LGPL-2.1.txt",
        "c7ce1276717604c29c5469179a2f368eac42846557be671bb52f4e16e884b2ad",
        26_934,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\Opus.txt",
        "cbd8bd07329a234413710c02b19d22c0cf78fc451882593e817130779b5455e3",
        1_989,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\PCRE2.md",
        "4195c519dcfe4a4ffedc4b8ccc5d49e4dd02efd5ece6b69a4fba5d20080902a9",
        4_115,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\libffi.txt",
        "e2a9b884b1c5d718efd6121457a1fbe7ece431352ca75c901260cef2e7d040e5",
        1_153,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\proxy-libintl.txt",
        "aa092600e3f475f0a55821fb9c529583d8e23e41bf0461254f02fcf94d3a32c2",
        25_774,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\sljit.txt",
        "37c7a090fc61388430d63fc97bf8a22ffce92d3ca2249aac572cfaf411d3070f",
        1_469,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\licenses\zlib.txt",
        "439c75ab12b340c5362b9d4b08ff05ec3a4d0eb7667a6fff49a9b16d8795c78e",
        1_024,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\manifest.json",
        "4959d21d559ad3adef3423f131dda7d824764973812e619b15c19eabb47e047e",
        4_773,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\runtime.json",
        "170ec9fb39e9748dace5abf4e8b8d49352b1bfed40f402eeb53138b5fca89f3e",
        16_894,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\sources.json",
        "e2f6b124f6277e7c3c4f5c7cc8830c66190f671ad65e972bb2cc9507dbfcb937",
        4_911,
    ),
    artifact(
        r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\windows-crt.json",
        "d16939bc85da395ded5411724fed84ffd36810b78d47c756cf094b0956bc3892",
        1_385,
    ),
];

/// The bundled `vcruntime140.dll` carries Microsoft's signature instead, so it
/// is admitted by its pinned hash alone.
pub(in crate::acp::bindings::codex) const AUTHENTICODE_SIGNED_FILES: [&str; 30] = [
    r"vendor\x86_64-pc-windows-msvc\bin\codex.exe",
    r"vendor\x86_64-pc-windows-msvc\bin\codex-code-mode-host.exe",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-command-runner.exe",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-windows-sandbox-setup.exe",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\codex-voice-host.exe",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gio-2.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\glib-2.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gmodule-2.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gobject-2.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstapp-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstapp.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudio-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudioconvert.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstaudioresample.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstbase-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstcoreelements.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstnet-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstopus.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstpbutils-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstreamer-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtp-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtp.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstrtpmanager.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gsttag-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\gstvideo-1.0-0.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\intl-8.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\libffi-8.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\opus.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\pcre2-8.dll",
    r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\z.dll",
];

pub(in crate::acp::bindings::codex) const AUTHENTICODE_UNSIGNED_FILES: [&str; 1] =
    [r"vendor\x86_64-pc-windows-msvc\codex-path\rg.exe"];

#[cfg(test)]
mod tests {
    use super::*;

    const MICROSOFT_SIGNED_FILES: [&str; 1] =
        [r"vendor\x86_64-pc-windows-msvc\codex-resources\voice\bin\vcruntime140.dll"];

    #[test]
    fn every_pinned_executable_has_exactly_one_signature_class() {
        let executables: Vec<&str> = PINNED_ARTIFACTS
            .iter()
            .map(|artifact| artifact.relative_path)
            .filter(|path| {
                let lower = path.to_ascii_lowercase();
                lower.ends_with(".exe") || lower.ends_with(".dll")
            })
            .collect();
        for path in &executables {
            let classes = [
                AUTHENTICODE_SIGNED_FILES.contains(path),
                AUTHENTICODE_UNSIGNED_FILES.contains(path),
                MICROSOFT_SIGNED_FILES.contains(path),
            ];
            assert_eq!(
                classes.iter().filter(|member| **member).count(),
                1,
                "{path}"
            );
        }
        for path in AUTHENTICODE_SIGNED_FILES
            .iter()
            .chain(&AUTHENTICODE_UNSIGNED_FILES)
            .chain(&MICROSOFT_SIGNED_FILES)
        {
            assert!(
                executables.contains(path),
                "{path} is not a pinned executable"
            );
        }
    }
}
