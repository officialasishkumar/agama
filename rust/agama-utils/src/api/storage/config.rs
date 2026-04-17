// Copyright (c) [2025] SUSE LLC
//
// All Rights Reserved.
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 2 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License for
// more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, contact SUSE LLC.
//
// To contact SUSE LLC about this file by physical or electronic mail, you may
// find current contact information at www.suse.com.

use merge::Merge;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::PartialSchema;

/// Storage configuration schema wrapper
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StorageSchema(pub Value);

impl utoipa::PartialSchema for StorageSchema {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::ObjectBuilder;
        use utoipa::openapi::extensions::Extensions;

        // Include the storage schema JSON at compile time
        const STORAGE_SCHEMA_JSON: &str =
            include_str!("../../../../agama-lib/share/storage.schema.json");

        // Parse the JSON schema
        let mut schema_value: serde_json::Value = serde_json::from_str(STORAGE_SCHEMA_JSON)
            .expect("Failed to parse storage.schema.json");

        // Fix all $ref paths to point to the component schema's $defs
        fix_refs(&mut schema_value, "#/components/schemas/storage.StorageSchema");

        // Extract the properties and $defs
        let obj = schema_value.as_object()
            .expect("Storage schema must be an object");

        let mut builder = ObjectBuilder::new();

        if let Some(desc) = obj.get("description").and_then(|v| v.as_str()) {
            builder = builder.description(Some(desc.to_string()));
        }

        // Add additional properties = false if specified
        if let Some(false) = obj.get("additionalProperties").and_then(|v| v.as_bool()) {
            builder = builder.additional_properties(Some(utoipa::openapi::schema::AdditionalProperties::FreeForm(false)));
        }

        // Convert properties
        if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
            for (key, value) in props {
                if let Ok(prop_schema) = serde_json::from_value::<utoipa::openapi::schema::Schema>(value.clone()) {
                    builder = builder.property(key, prop_schema);
                }
            }
        }

        // Add $defs as an extension (OpenAPI 3.1 supports this)
        if let Some(defs) = obj.get("$defs") {
            let extensions: Extensions = [(String::from("$defs"), defs.clone())]
                .into_iter()
                .collect();
            builder = builder.extensions(Some(extensions));
        }

        utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(builder.build()))
    }
}

/// Fix all `#/$defs/X` references to point to the component schema's path
fn fix_refs(value: &mut serde_json::Value, schema_path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(ref_value) = map.get_mut("$ref") {
                if let Some(ref_str) = ref_value.as_str() {
                    if let Some(def_name) = ref_str.strip_prefix("#/$defs/") {
                        *ref_value = serde_json::Value::String(
                            format!("{}/$defs/{}", schema_path, def_name)
                        );
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                fix_refs(v, schema_path);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                fix_refs(item, schema_path);
            }
        }
        _ => {}
    }
}

impl utoipa::ToSchema for StorageSchema {
    fn name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("storage.StorageSchema")
    }

    fn schemas(
        schemas: &mut Vec<(String, utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>)>,
    ) {
        // Just register the main schema with all $defs inline
        schemas.push((
            Self::name().to_string(),
            Self::schema(),
        ));
    }
}


#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = storage::Config)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<StorageSchema>)]
    pub storage: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_autoyast_storage: Option<Value>,
}

impl Config {
    pub fn has_value(&self) -> bool {
        self.storage.is_some() || self.legacy_autoyast_storage.is_some()
    }
}

impl Merge for Config {
    fn merge(&mut self, other: Self) {
        if let Some(storage) = &mut self.storage {
            if let Some(other_storage) = other.storage {
                merge_values_as_objects(storage, other_storage);
            }
        } else {
            self.storage = other.storage;
        }

        // No need to merge both values because it is just an array of drives.
        if self.legacy_autoyast_storage.is_none() {
            self.legacy_autoyast_storage = other.legacy_autoyast_storage;
        }
    }
}

// Merge to serde_json::Value structs.
//
// Both Value structs are supposed to represent JSON objects.
fn merge_values_as_objects(left: &mut Value, right: Value) {
    let Value::Object(left_object) = left else {
        return;
    };

    let Value::Object(right_object) = right else {
        return;
    };

    for (k, v) in right_object {
        left_object.entry(k).or_insert(v);
    }
}

#[cfg(test)]
mod tests {
    use merge::Merge;

    use super::*;

    #[test]
    fn test_merge_with_default_config() {
        let mut config: Config = serde_json::from_str(r#"{ "storage": { "drives": [] }}"#).unwrap();
        let original = Config::default();

        config.merge(original);
        assert!(config.storage.is_some());
    }

    #[test]
    fn test_merge_storage_key() {
        let mut config: Config = serde_json::from_str(r#"{ "storage": { "drives": [] }}"#).unwrap();
        let original: Config = serde_json::from_str(r#"{ "storage": { "mdRaids": [] }}"#).unwrap();

        config.merge(original);
        let value = config.storage.unwrap();
        assert!(value.get("drives").is_some());
        assert!(value.get("mdRaids").is_some());
    }

    #[test]
    fn test_merge_with_no_storage() {
        let mut config = Config::default();
        let original: Config = serde_json::from_str(r#"{ "storage": { "drives": [] }}"#).unwrap();

        config.merge(original);
        assert!(config.storage.is_some());
    }
}
