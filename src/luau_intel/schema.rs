//! The reflection database for the Luau intelligence (src/luau_intel):
//! the few questions index.rs asks of it (from Android-rbxl's schema.rs).
#![allow(dead_code)]

use rbx_reflection::{DataType, PropertyKind};
use rbx_reflection_database::get as get_reflection_database;
use std::collections::BTreeMap;

/// In newer `rbx_reflection_database`, `get()` returns a `Result` because the
/// embedded database can (theoretically) fail to load. On every shipped build
/// it resolves, so fall back to an empty database on error — callers treat a
/// missing class/enum the same as "not found".
fn database() -> &'static rbx_reflection::ReflectionDatabase<'static> {
    match get_reflection_database() {
        Ok(db) => db,
        Err(_) => {
            // No global const empty database is exposed; use a leaked empty one
            // so the signature stays a simple shared reference.
            static EMPTY: std::sync::OnceLock<rbx_reflection::ReflectionDatabase<'static>> =
                std::sync::OnceLock::new();
            EMPTY.get_or_init(rbx_reflection::ReflectionDatabase::new)
        }
    }
}

pub fn class_exists(class_name: &str) -> bool {
    database().classes.contains_key(class_name)
}

pub fn class_is_service(class_name: &str) -> bool {
    database().classes.get(class_name)
        .is_some_and(|desc| desc.tags.contains(&rbx_reflection::ClassTag::Service))
}

pub fn class_is_creatable(class_name: &str) -> bool {
    database().classes.get(class_name).is_some_and(|desc| {
        !desc.tags.contains(&rbx_reflection::ClassTag::NotCreatable)
            && !desc.tags.contains(&rbx_reflection::ClassTag::Service)
    })
}

pub fn enum_item_exists(enum_name: &str, item_name: &str) -> Option<bool> {
    database().enums.get(enum_name)
        .map(|desc| desc.items.contains_key(item_name))
}

/// Retrieve all available schema properties and their official types for a class
pub fn get_class_schema_properties(class_name: &str) -> BTreeMap<String, String> {
    let db = database();
    let mut out = BTreeMap::new();

    let mut current_class = Some(class_name);
    while let Some(cls) = current_class {
        if let Some(desc) = db.classes.get(cls) {
            for (prop_name, prop_desc) in &desc.properties {
                let type_name = format!("{:?}", prop_desc.data_type);
                out.entry(prop_name.to_string()).or_insert(type_name);
            }
            current_class = desc.superclass.as_deref();
        } else {
            break;
        }
    }

    out
}

/// One class up, preferring the reflection database's link and falling back to
/// the built-in API dump. The database omits abstract classes - `Part` reaches
/// `BasePart` through `FormFactorPart`, which it does not describe - and a walk
/// that stops at that gap loses every property inherited above it.
fn superclass_of(class: &str) -> Option<&'static str> {
    if let Some(desc) = database().classes.get(class) {
        if let Some(parent) = desc.superclass {
            return Some(parent);
        }
    }
    super::api_data::class_superclass(class)
}

/// `class` and each of its ancestors, deepest first. A missing entry does not end
/// the chain (see [`superclass_of`]), and the depth limit is a cycle guard rather
/// than a real bound: no Roblox class is more than a handful of hops from Instance.
fn class_chain(class_name: &str) -> Vec<&str> {
    let mut chain: Vec<&str> = Vec::with_capacity(16);
    let mut current = Some(class_name);
    for _ in 0..64 {
        let Some(name) = current else { break };
        if chain.contains(&name) {
            break;
        }
        chain.push(name);
        current = superclass_of(name);
    }
    chain
}

/// Resolve the official type of a property on a class, walking the superclass
/// chain. Returns the reflection `DataType` (either a concrete `VariantType` or
/// a named Roblox `Enum`), so the properties editor can render the right
/// widget (e.g. a dropdown for enums instead of a raw integer).
pub fn resolve_property_type(class_name: &str, prop_name: &str) -> Option<DataType<'static>> {
    let db = database();
    for cls in class_chain(class_name) {
        if let Some(desc) = db.classes.get(cls) {
            if let Some(pd) = desc.properties.get(prop_name) {
                return Some(pd.data_type.clone());
            }
            // The database carries an entry per spelling, with the legacy one
            // (`size`) marked as an alias of the canonical property. A lookup by
            // either name therefore has to land on the same descriptor, which the
            // case-insensitive retry below arranges; following the alias keeps the
            // type of an alias identical to its target even if the database ever
            // records them differently.
            if let Some(pd) = desc
                .properties
                .iter()
                .find(|(key, property)| {
                    let key: &str = *key;
                    key.eq_ignore_ascii_case(prop_name)
                        || property.name.eq_ignore_ascii_case(prop_name)
                })
                .map(|(_, property)| property)
            {
                if let PropertyKind::Alias { alias_for } = &pd.kind {
                    if let Some(target) = desc.properties.get(*alias_for) {
                        return Some(target.data_type.clone());
                    }
                }
                return Some(pd.data_type.clone());
            }
        }
    }
    None
}
