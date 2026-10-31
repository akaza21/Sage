use std::{
    io::{self, Read},
    sync::atomic::AtomicBool,
};

use super::{
    MAX_PASSWORD_BYTES, VaultError,
    csv_import::{self, MAX_CREDENTIALS, ParsedCsv},
};

fn read(source: impl Read) -> Result<ParsedCsv, VaultError> {
    csv_import::read(source, &AtomicBool::new(false))
}

fn parse(csv: &str) -> ParsedCsv {
    read(csv.as_bytes()).unwrap()
}

fn rows(parsed: &ParsedCsv) -> Vec<(&str, &str, &str)> {
    parsed
        .credentials
        .iter()
        .map(|credential| {
            (
                credential.origin.as_str(),
                credential.username.as_str(),
                credential.password.expose(),
            )
        })
        .collect()
}

/// Endless unusable CSV rows, for files that grow past the size cap mid-read
/// without reaching the credential cap first.
struct Endless;

impl Read for Endless {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let row = b"not a site,user,secret\n";
        for chunk in buffer.chunks_mut(row.len()) {
            chunk.copy_from_slice(&row[..chunk.len()]);
        }
        Ok(buffer.len())
    }
}

#[test]
fn each_password_manager_header_maps_to_site_username_and_password() {
    let exports = [
        "name,url,username,password,note\nx,https://a.example/,ada,pw,\n",
        "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\"\n\"https://a.example\",\"ada\",\"pw\",,\"https://a.example\",\"{1}\"\n",
        "Title,URL,Username,Password,Notes,OTPAuth\nx,https://a.example/login,ada,pw,,\n",
        "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n,,login,x,,,0,https://a.example,ada,pw,\n",
        "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\nx,https://a.example,ada,pw,,false,false,,\n",
    ];
    for export in exports {
        let parsed = parse(export);
        assert_eq!(
            rows(&parsed),
            [("https://a.example", "ada", "pw")],
            "{export}"
        );
        assert_eq!(parsed.skipped, 0, "{export}");
    }
}

#[test]
fn quoted_fields_a_bom_and_crlf_lines_parse_exactly() {
    let parsed = parse(
        "\u{feff}url,username,password\r\n\"https://a.example\",\"ada, lovelace\",\"p\"\"w,\nnext\"\r\n",
    );
    assert_eq!(
        rows(&parsed),
        [("https://a.example", "ada, lovelace", "p\"w,\nnext")]
    );
}

#[test]
fn unusable_rows_are_skipped_and_counted() {
    let long_password = "p".repeat(MAX_PASSWORD_BYTES + 1);
    let csv = format!(
        "url,username,password,httpRealm\n\
         https://ok.example,ada,pw,\n\
         android://hash@com.example/,ada,pw,\n\
         https://realm.example,ada,pw,Admin area\n\
         https://empty.example,ada,,\n\
         https://long.example,ada,{long_password},\n\
         not a url,ada,pw,\n\
         https://short.example\n"
    );
    let parsed = parse(&csv);
    assert_eq!(rows(&parsed), [("https://ok.example", "ada", "pw")]);
    assert_eq!(parsed.skipped, 6);
}

#[test]
fn invalid_utf8_rows_are_skipped() {
    let mut csv = b"url,username,password\nhttps://a.example,ada,".to_vec();
    csv.extend_from_slice(b"\xff\xfe\n");
    csv.extend_from_slice(b"https://b.example,grace,pw\n");
    let parsed = read(csv.as_slice()).unwrap();
    assert_eq!(rows(&parsed), [("https://b.example", "grace", "pw")]);
    assert_eq!(parsed.skipped, 1);
}

#[test]
fn an_oversized_record_is_skipped_without_losing_the_rows_after_it() {
    let huge = "x".repeat(20 * 1024);
    let csv = format!(
        "url,username,password\nhttps://a.example,ada,\"{huge}\"\nhttps://b.example,grace,pw\n"
    );
    let parsed = parse(&csv);
    assert_eq!(rows(&parsed), [("https://b.example", "grace", "pw")]);
    assert_eq!(parsed.skipped, 1);
    let many_fields = format!(
        "url,username,password\n{}\nhttps://b.example,grace,pw\n",
        ",".repeat(100)
    );
    let parsed = parse(&many_fields);
    assert_eq!(rows(&parsed), [("https://b.example", "grace", "pw")]);
    assert_eq!(parsed.skipped, 1);
}

#[test]
fn a_record_is_capped_by_its_raw_bytes_not_its_decoded_size() {
    // 10 KiB of escaped quotes decode to 5 KiB, still past the raw cap.
    let escaped = "\"\"".repeat(5 * 1024);
    let csv = format!(
        "url,username,password\nhttps://a.example,ada,\"{escaped}\"\nhttps://b.example,grace,pw\n"
    );
    let parsed = parse(&csv);
    assert_eq!(rows(&parsed), [("https://b.example", "grace", "pw")]);
    assert_eq!(parsed.skipped, 1);
}

#[test]
fn a_file_without_the_needed_columns_is_refused() {
    for csv in [
        "",
        "name,notes\nx,y\n",
        "url,username\nhttps://a.example,ada\n",
    ] {
        assert!(
            matches!(read(csv.as_bytes()), Err(VaultError::Import(_))),
            "{csv:?}"
        );
    }
    let oversized_header = format!("url,username,password,{}\n", "h".repeat(9 * 1024));
    assert!(matches!(
        read(oversized_header.as_bytes()),
        Err(VaultError::Import(_))
    ));
}

#[test]
fn a_file_that_grows_past_the_size_cap_is_refused_part_way() {
    let header = &b"url,username,password\n"[..];
    match read(header.chain(Endless)) {
        Err(VaultError::Import(message)) => assert!(message.contains("16 MB"), "{message}"),
        _ => panic!("growing file was not refused"),
    }
}

#[test]
fn one_credential_past_the_count_cap_refuses_the_import() {
    let mut csv = String::from("url,username,password\n");
    for index in 0..MAX_CREDENTIALS {
        csv.push_str(&format!("https://a.example,user{index},pw\n"));
    }
    assert_eq!(parse(&csv).credentials.len(), MAX_CREDENTIALS);
    csv.push_str("https://a.example,one-more,pw\n");
    assert!(matches!(read(csv.as_bytes()), Err(VaultError::Import(_))));
}

#[test]
fn a_cancelled_read_stops_before_the_next_chunk() {
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        csv_import::read(Endless, &cancelled),
        Err(VaultError::Cancelled)
    ));
}
