//! Narrow Roblox XML importer for visual-reference reconstruction.
//!
//! This crate intentionally emits a tool-owned intermediate scene instead of a
//! Cubacadabra package manifest. It preserves source facts first; package and
//! renderer adaptation can then be measured against that stable artifact.

use rbx_dom_weak::types::{CFrame, Color3, ContentType, Matrix3, Variant, Vector3};
use rbx_dom_weak::{Instance, WeakDom, types::Ref, ustr};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{BufReader, Write},
    path::{Path, PathBuf},
};

mod collision;
mod importer;
mod mesh_export;
mod mesh_overrides;
mod model;

pub use importer::{import_reference, read_reference_scene};
pub use mesh_export::export_reference_mesh;
#[cfg(test)]
pub(crate) use mesh_export::{
    StaticMeshGroup, StaticMeshVertex, append_static_geometry, write_static_glb,
};
pub use model::*;

#[cfg(test)]
mod tests;
