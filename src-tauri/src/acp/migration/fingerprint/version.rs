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
        16 => SchemaClass::CurrentV16,
        17 => SchemaClass::CurrentV17,
        18 => SchemaClass::CurrentV18,
        19 => SchemaClass::CurrentV19,
        20 => SchemaClass::CurrentV20,
        21 => SchemaClass::CurrentV21,
        22 => SchemaClass::CurrentV22,
        23 => SchemaClass::CurrentV23,
        version if version == super::super::session_resume_link::VERSION => {
            SchemaClass::CurrentResumeLink
        }
        version if version == super::super::title_source::VERSION => {
            SchemaClass::CurrentTitleSource
        }
        version if version == super::super::session_facts_turn_index::VERSION => {
            SchemaClass::CurrentSessionFactsTurnIndex
        }
        version if version == super::super::operation_terminal_index::VERSION => {
            SchemaClass::CurrentOperationTerminalIndex
        }
        _ => return invalid("Sage schema version is above the supported maximum"),
    })
}
