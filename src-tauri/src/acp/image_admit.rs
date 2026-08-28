use crate::acp::{
    contract::SafeText,
    intent::{IMAGE_BASE64_MAX, IMAGE_DECODED_BYTES_MAX, ImageMediaType, NormalizedPayload},
    limits::LIMITS,
};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageAdmitError {
    MissingBytes,
    InvalidBase64,
    UnsupportedMediaType,
    Oversize,
}

impl ImageAdmitError {
    pub(crate) fn unsupported_summary(self) -> &'static str {
        match self {
            Self::MissingBytes => "Image payload is missing",
            Self::InvalidBase64 => "Image payload is not valid base64",
            Self::UnsupportedMediaType => "Image media type is not admitted",
            Self::Oversize => "Image exceeds the bounded size cap",
        }
    }
}

pub(crate) fn admit_image_data_url(url: &str) -> Result<NormalizedPayload, ImageAdmitError> {
    let (media_type, data) = split_data_url(url)?;
    admit_image_parts(media_type, data)
}

pub(crate) fn admit_image_parts(
    media_type: ImageMediaType,
    encoded: &str,
) -> Result<NormalizedPayload, ImageAdmitError> {
    let bytes = decode_std_base64(encoded)?;
    let data = encode_std_base64(&bytes);
    let payload = NormalizedPayload::Image {
        media_type,
        data: SafeText::new("imageData", data).map_err(|_| ImageAdmitError::Oversize)?,
    };
    if serialized_item_bytes(&payload) > LIMITS.wire.normalized_item_bytes as usize {
        return Err(ImageAdmitError::Oversize);
    }
    Ok(payload)
}

pub(crate) fn image_data_url(
    media_type: ImageMediaType,
    data: &str,
) -> Result<SafeText<262144>, crate::acp::contract::ContractError> {
    SafeText::new(
        "dataUrl",
        format!("data:{};base64,{data}", media_type.as_str()),
    )
}

fn split_data_url(url: &str) -> Result<(ImageMediaType, &str), ImageAdmitError> {
    const PREFIX: &str = "data:";
    const MARKER: &str = ";base64,";
    if url.is_empty() {
        return Err(ImageAdmitError::MissingBytes);
    }
    if looks_like_remote_or_path(url) {
        return Err(ImageAdmitError::UnsupportedMediaType);
    }
    let rest = url
        .strip_prefix(PREFIX)
        .ok_or(ImageAdmitError::UnsupportedMediaType)?;
    let (media, data) = rest
        .split_once(MARKER)
        .ok_or(ImageAdmitError::UnsupportedMediaType)?;
    if media.contains(';') || media.contains(',') {
        return Err(ImageAdmitError::UnsupportedMediaType);
    }
    let media_type = ImageMediaType::parse(media).ok_or(ImageAdmitError::UnsupportedMediaType)?;
    if data.is_empty() {
        return Err(ImageAdmitError::MissingBytes);
    }
    Ok((media_type, data))
}

fn looks_like_remote_or_path(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("javascript:")
        || lower.starts_with("blob:")
        || lower.starts_with("file:")
        || lower.starts_with("\\\\")
        || (url.as_bytes().get(1) == Some(&b':')
            && url.as_bytes().first().is_some_and(u8::is_ascii_alphabetic))
}

fn decode_std_base64(input: &str) -> Result<Vec<u8>, ImageAdmitError> {
    if input.is_empty() {
        return Err(ImageAdmitError::MissingBytes);
    }
    if !input.len().is_multiple_of(4) || input.len() > IMAGE_BASE64_MAX {
        return Err(if input.len() > IMAGE_BASE64_MAX {
            ImageAdmitError::Oversize
        } else {
            ImageAdmitError::InvalidBase64
        });
    }
    let padding = input
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return Err(ImageAdmitError::InvalidBase64);
    }
    let decoded_len = (input.len() / 4) * 3 - padding;
    if decoded_len == 0 {
        return Err(ImageAdmitError::MissingBytes);
    }
    if decoded_len > IMAGE_DECODED_BYTES_MAX {
        return Err(ImageAdmitError::Oversize);
    }
    let bytes = input.as_bytes();
    if let Some(first_eq) = bytes.iter().position(|byte| *byte == b'=')
        && (first_eq < bytes.len() - padding || bytes[first_eq..].iter().any(|byte| *byte != b'='))
    {
        return Err(ImageAdmitError::InvalidBase64);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(decoded_len)
        .map_err(|_| ImageAdmitError::Oversize)?;
    for chunk in bytes.chunks_exact(4) {
        let n0 = decode_digit(chunk[0])?;
        let n1 = decode_digit(chunk[1])?;
        let n2 = if chunk[2] == b'=' {
            0
        } else {
            decode_digit(chunk[2])?
        };
        let n3 = if chunk[3] == b'=' {
            0
        } else {
            decode_digit(chunk[3])?
        };
        output.push((n0 << 2) | (n1 >> 4));
        if chunk[2] != b'=' {
            output.push((n1 << 4) | (n2 >> 2));
        }
        if chunk[3] != b'=' {
            output.push((n2 << 6) | n3);
        }
    }
    if output.len() != decoded_len {
        output.clear();
        return Err(ImageAdmitError::InvalidBase64);
    }
    Ok(output)
}

fn decode_digit(byte: u8) -> Result<u8, ImageAdmitError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(ImageAdmitError::InvalidBase64),
    }
}

fn encode_std_base64(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        output.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(n & 63) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn serialized_item_bytes(payload: &NormalizedPayload) -> usize {
    serde_json::to_vec(payload)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

    #[test]
    fn admits_png_data_url_without_retaining_a_remote_url() {
        let url = format!("data:image/png;base64,{PNG_1X1}");
        let NormalizedPayload::Image { media_type, data } = admit_image_data_url(&url).unwrap()
        else {
            panic!("expected image payload");
        };
        assert_eq!(media_type, ImageMediaType::Png);
        assert_eq!(data.as_str(), PNG_1X1);
    }

    #[test]
    fn rejects_svg_html_empty_and_unknown_media_types() {
        for url in [
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:text/html;base64,PGgxPjwvaDE+",
            "data:image/gif;base64,AAAA",
            "data:;base64,AAAA",
        ] {
            assert_eq!(
                admit_image_data_url(url),
                Err(ImageAdmitError::UnsupportedMediaType)
            );
        }
    }

    #[test]
    fn rejects_http_javascript_and_filesystem_sources() {
        for url in [
            "https://example.com/x.png",
            "http://127.0.0.1/x.png",
            "javascript:alert(1)",
            r"C:\tmp\x.png",
            "file:///tmp/x.png",
        ] {
            assert_eq!(
                admit_image_data_url(url),
                Err(ImageAdmitError::UnsupportedMediaType)
            );
        }
    }

    #[test]
    fn rejects_missing_or_invalid_base64_without_keeping_bytes() {
        assert_eq!(
            admit_image_data_url("data:image/png;base64,"),
            Err(ImageAdmitError::MissingBytes)
        );
        assert_eq!(
            admit_image_data_url("data:image/png;base64,!!!!"),
            Err(ImageAdmitError::InvalidBase64)
        );
        assert_eq!(
            admit_image_parts(ImageMediaType::Png, ""),
            Err(ImageAdmitError::MissingBytes)
        );
    }

    #[test]
    fn rejects_oversize_decoded_bytes_without_a_partial_image() {
        let oversize = encode_std_base64(&vec![0_u8; IMAGE_DECODED_BYTES_MAX + 1]);
        assert_eq!(
            admit_image_parts(ImageMediaType::Png, &oversize),
            Err(ImageAdmitError::Oversize)
        );
        assert!(oversize.len() > IMAGE_BASE64_MAX || oversize.len().is_multiple_of(4));
    }
}
