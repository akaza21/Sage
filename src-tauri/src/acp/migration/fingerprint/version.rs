use super::{SchemaClass, invalid};
use crate::error::AppResult;

pub(super) fn class_for_version(version: u32) -> AppResult<SchemaClass> {
    Ok(match version {
        1 => SchemaClass::LegacyV1,
        2 => SchemaClass::LegacyV2,
        3 => SchemaClass::LegacyV3,
        4 => SchemaClass::LegacyV4,
        5 => SchemaClass::CurrentV5,
        6 => SchemaClass::CurrentV6,
        7 => SchemaClass::CurrentV7,
        8 => SchemaClass::CurrentV8,
        9 => SchemaClass::CurrentV9,
        10 => SchemaClass::CurrentV10,
        11 => SchemaClass::CurrentV11,
        12 => SchemaClass::CurrentV12,
        13 => SchemaClass::CurrentV13,
        14 => SchemaClass::CurrentV14,
        15 => SchemaClass::CurrentV15,
        _ => return invalid("Sage schema version is above the supported maximum"),
    })
}
