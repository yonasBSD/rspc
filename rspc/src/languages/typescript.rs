use std::{borrow::Cow, collections::BTreeMap, iter::once, path::Path};

use serde_json::json;
use specta::{datatype::DataType, NamedType, Type};
use specta_typescript::{datatype, export_named_datatype, ExportError};

use crate::{
    procedure::ProcedureType, types::TypesOrType, util::literal_object, ProcedureKind, Types,
};

pub struct Typescript {
    inner: specta_typescript::Typescript,
    generate_source_maps: bool,
}

// TODO: Traits - `Debug`, `Clone`, etc

impl Default for Typescript {
    fn default() -> Self {
        Self {
            inner: specta_typescript::Typescript::default().framework_header("// This file was generated by [rspc](https://github.com/specta-rs/rspc). Do not edit this file manually."),
            generate_source_maps: false,
        }
    }
}

impl Typescript {
    pub fn header(self, header: impl Into<Cow<'static, str>>) -> Self {
        Self {
            inner: self.inner.header(header),
            ..self
        }
    }

    pub fn enable_source_maps(mut self) -> Self {
        self.generate_source_maps = true;
        self
    }

    // TODO: Clone all methods

    pub fn export_to(&self, path: impl AsRef<Path>, types: &Types) -> Result<(), ExportError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut bindings = self.export(types)?;
        if self.generate_source_maps {
            println!(
                "WARNING: Typescript source maps in an unstable feature. Use at your own discretion!"
            );

            bindings += "export { Procedures } from './bindings_t';";
        } else {
            generate_bindings(&mut bindings, self, types, |_, _, _| {});
        }
        std::fs::write(&path, bindings)?;
        self.inner.format(&path)?;

        if self.generate_source_maps {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string(); // TODO: Error handling
            let d_ts_file_name = format!("{stem}_t.d.ts");
            let d_ts_map_file_name = format!("{stem}_t.d.ts.map");
            let d_ts_path = path.parent().unwrap().join(&d_ts_file_name); // TODO: Error handling
            let d_ts_map_path = path.parent().unwrap().join(&d_ts_map_file_name); // TODO: Error handling

            // let mut bindings = construct_file(self);
            // bindings += "export * from './bindings_t';";

            let mut source_map = SourceMap::default();
            let mut d_ts_file = construct_file(self);
            generate_bindings(&mut d_ts_file, self, types, |name, pos, procedure_type| {
                source_map.insert(
                    name.to_string(),
                    pos,
                    (
                        // TODO: Don't cast
                        procedure_type.location.line() as usize,
                        procedure_type.location.column() as usize,
                    ),
                    procedure_type.location.file().to_string(),
                );
            });
            d_ts_file += &format!("\n//# sourceMappingURL={d_ts_map_file_name}");

            // std::fs::write(&path, bindings)?;
            std::fs::write(&d_ts_path, d_ts_file)?;
            std::fs::write(
                &d_ts_map_path,
                source_map.generate(
                    d_ts_file_name.into(),
                    "".into(), // TODO
                ),
            )?;

            self.inner.format(&d_ts_path)?;
        }

        Ok(())
    }

    pub fn export(&self, types: &Types) -> Result<String, ExportError> {
        let mut typess = types.types.clone();

        #[cfg(not(feature = "nolegacy"))]
        {
            let legacy_types =
                crate::legacy::interop::construct_legacy_bindings_type(&types.procedures);

            #[derive(Type)]
            struct ProceduresLegacy;

            let s = literal_object(
                "ProceduresLegacy".into(),
                Some(ProceduresLegacy::sid()),
                legacy_types.into_iter(),
            );
            let mut ndt = ProceduresLegacy::definition_named_data_type(&mut typess);
            ndt.inner = s.into();
            typess.insert(ProceduresLegacy::sid(), ndt);
        }

        self.inner.export(&typess)
    }

    // pub fn export_ // TODO: Source map (can we make it be inline?)
}

fn generate_bindings(
    out: &mut String,
    this: &Typescript,
    types: &Types,
    mut on_procedure: impl FnMut(&Cow<'static, str>, (usize, usize), &ProcedureType),
) {
    fn inner(
        out: &mut String,
        this: &Typescript,
        on_procedure: &mut impl FnMut(&Cow<'static, str>, (usize, usize), &ProcedureType),
        types: &Types,
        source_pos: (usize, usize),
        key: &Cow<'static, str>,
        item: &TypesOrType,
    ) {
        match item {
            TypesOrType::Type(procedure_type) => {
                on_procedure(&key, source_pos, procedure_type);

                // *out += "\t"; // TODO: Correct padding
                *out += "{ kind: ";
                *out += match procedure_type.kind {
                    ProcedureKind::Query => r#""query""#,
                    ProcedureKind::Mutation => r#""mutation""#,
                    ProcedureKind::Subscription => r#""subscription""#,
                };

                *out += ", input: ";
                *out += &datatype(
                    &this.inner,
                    &specta::datatype::FunctionResultVariant::Value(procedure_type.input.clone()),
                    &types.types,
                )
                .unwrap(); // TODO: Error handling

                *out += ", output: ";
                *out += &datatype(
                    &this.inner,
                    &specta::datatype::FunctionResultVariant::Value(procedure_type.output.clone()),
                    &types.types,
                )
                .unwrap(); // TODO: Error handling

                *out += ", error: ";
                *out += &datatype(
                    &this.inner,
                    &specta::datatype::FunctionResultVariant::Value(procedure_type.error.clone()),
                    &types.types,
                )
                .unwrap(); // TODO: Error handling

                *out += " }";
            }
            TypesOrType::Types(btree_map) => {
                // TODO: Jump to definition on routers
                // *out += "name: ";

                *out += "{\n";

                for (key, item) in btree_map.iter() {
                    *out += "\t";

                    let source_pos = get_current_pos(out);

                    *out += key;
                    *out += ": ";
                    inner(out, this, on_procedure, types, source_pos, key, &item);
                    *out += ",\n";
                }

                *out += "}";
            }
        }
    }

    *out += "export type Procedures = ";
    inner(
        out,
        this,
        &mut on_procedure,
        types,
        // We know this is only used in `TypesOrType::Type` and we don't parse that so it's value means nothing.
        (0, 0),
        // We know this is only used in `TypesOrType::Type` and we don't parse that so it's value means nothing.
        &"".into(),
        &TypesOrType::Types(types.procedures.clone()),
    );
}

fn construct_file(this: &Typescript) -> String {
    let mut out = this.inner.header.to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out += &this.inner.framework_header;
    out.push_str("\n\n");
    out
}

#[derive(Default)]
struct SourceMap {
    mappings: BTreeMap<usize, Vec<(usize, usize, usize, (usize, usize))>>,
    sources: Vec<String>,
    names: Vec<String>,
}

impl SourceMap {
    pub fn insert(
        &mut self,
        name: String,
        (generated_line, generated_col): (usize, usize),
        source_pos: (usize, usize),
        source_file: String,
    ) {
        if !self.sources.contains(&source_file) {
            self.sources.push(source_file.clone());
        }
        let source_id = self.sources.iter().position(|s| *s == source_file).unwrap();

        if !self.names.contains(&name) {
            self.names.push(name.clone());
        }
        let name_id = self.names.iter().position(|s| *s == name).unwrap();

        self.mappings
            .entry(generated_line)
            .or_insert(Default::default())
            .push((generated_col, source_id, name_id, source_pos));
    }

    pub fn generate(&self, file: Cow<'static, str>, source_base_path: Cow<'static, str>) -> String {
        let mut mappings = String::new();
        let mut last_source_line = None::<usize>;
        let mut last_source_col = None::<usize>;
        let mut last_source_file = None::<usize>;
        let mut last_name_id = None::<usize>;

        for i in 1..((self.mappings.keys().max().copied().unwrap_or(0)) + 1) {
            let mut last_col = None::<usize>;

            if let Some(line_mappings) = self.mappings.get(&i) {
                for (
                    i,
                    (
                        actual_col,
                        actual_source_file,
                        actual_name_id,
                        (actual_source_line, actual_source_col),
                    ),
                ) in line_mappings.iter().enumerate()
                {
                    if i != 0 {
                        mappings.push(',');
                    }

                    let col = last_col.map(|l| actual_col - l).unwrap_or(*actual_col);
                    last_col = Some(*actual_col);

                    let actual_source_line = *actual_source_line - 1;
                    let source_line = last_source_line
                        .map(|l| actual_source_line - l)
                        .unwrap_or(actual_source_line);
                    last_source_line = Some(actual_source_line);

                    let source_col = last_source_col
                        .map(|l| actual_source_col - l)
                        .unwrap_or(*actual_source_col);
                    last_source_col = Some(*actual_source_col);

                    let source_file = last_source_file
                        .map(|l| actual_source_file - l)
                        .unwrap_or(*actual_source_file);
                    last_source_file = Some(*actual_source_file);

                    let name_id = last_name_id
                        .map(|l| actual_name_id - l)
                        .unwrap_or(*actual_name_id);
                    last_name_id = Some(*actual_name_id);

                    // TODO: Don't integer cast
                    let input = [
                        col as i64,
                        source_file as i64,
                        source_line as i64,
                        source_col as i64,
                        name_id as i64,
                    ];

                    mappings.push_str(&generate_vlq_segment(&input));
                }
            };

            mappings.push(';');
        }

        serde_json::to_string(&json!({
            "version": 3,
            "file": file,
            "sources":self.sources
                .iter()
                .map(|n| format!("{source_base_path}{n}"))
                .collect::<Vec<_>>(),
            "names": self.names,
            "mappings": mappings
        }))
        .expect("failed to generate source map")
    }
}

fn get_current_pos(s: &String) -> (usize, usize) {
    (s.split("\n").count(), s.split("\n").last().unwrap().len())
}

// Following copied from: https://docs.rs/sourcemap/latest/src/sourcemap/vlq.rs.html#307-313

/// Encodes a VLQ segment from a slice.
pub fn generate_vlq_segment(nums: &[i64]) -> String {
    let mut rv = String::new();
    for &num in nums {
        encode_vlq(&mut rv, num);
    }
    rv
}

const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub(crate) fn encode_vlq(out: &mut String, num: i64) {
    let mut num = if num < 0 { ((-num) << 1) + 1 } else { num << 1 };

    loop {
        let mut digit = num & 0b11111;
        num >>= 5;
        if num > 0 {
            digit |= 1 << 5;
        }
        out.push(B64_CHARS[digit as usize] as char);
        if num == 0 {
            break;
        }
    }
}
